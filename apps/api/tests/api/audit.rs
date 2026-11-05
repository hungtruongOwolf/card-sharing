//! T-17: append-only audit log for auth events (SR-33), against real Postgres + Redis
//! (TEST_DATABASE_URL, TEST_REDIS_URL; skipped locally when unset, required in CI).

use crate::common::*;
use axum::body::Body;
use http::{Request, StatusCode};
use identity::RevokeReason;
use identity::testing::random_nonce;
use platform::audit::{Actor, AuditAction, AuditEntry, record};
use platform::clock::Clock;
use serde_json::{Value, json};
use time::{Date, Duration, Month};
use uuid::Uuid;

/// Every audit row of `account` as JSON (all columns), oldest first.
async fn rows_of(env: &TestEnv, account: Uuid) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT to_jsonb(a) FROM platform.audit_events a WHERE actor_id = $1 ORDER BY at, id",
    )
    .bind(account)
    .fetch_all(env.identity.pool())
    .await
    .unwrap()
}

fn actions(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .map(|row| row["action"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn application_role_may_insert_but_never_update_or_delete() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let actor = Uuid::now_v7();
    let entry = AuditEntry::new(Actor::User(actor), AuditAction::SignIn);
    let now = env.clock.now();
    let pool = env.identity.pool();

    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE app_rw")
        .execute(&mut *tx)
        .await
        .unwrap();
    record(&mut *tx, &entry, now).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(rows_of(&env, actor).await.len(), 1);

    for statement in [
        "UPDATE platform.audit_events SET action = 'auth.forged' WHERE actor_id = $1",
        "DELETE FROM platform.audit_events WHERE actor_id = $1",
    ] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE app_rw")
            .execute(&mut *tx)
            .await
            .unwrap();
        let err = sqlx::query(statement)
            .bind(actor)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        let code = err.as_database_error().and_then(|e| e.code());
        assert_eq!(code.as_deref(), Some("42501"), "{statement}: {err}");
        tx.rollback().await.unwrap();
    }
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE app_rw")
        .execute(&mut *tx)
        .await
        .unwrap();
    let truncate = sqlx::query("TRUNCATE platform.audit_events")
        .execute(&mut *tx)
        .await;
    assert!(truncate.is_err(), "TRUNCATE must be denied");
    tx.rollback().await.unwrap();
    assert_eq!(rows_of(&env, actor).await.len(), 1, "row unchanged");
}

#[tokio::test]
async fn sign_in_is_audited_without_tokens_or_emails() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let nonce = random_nonce();
    let subject = format!("audit-{}", Uuid::now_v7());
    let mut claims = env.apple_claims(&subject, &nonce);
    let email = format!("{}@example.com", Uuid::now_v7().simple());
    claims["email"] = json!(email);
    let id_token = identity::testing::TestRsaKey::shared().sign(&claims);
    let response = send(
        &env.router,
        post_json(
            "/v1/auth/apple",
            None,
            &json!({ "id_token": id_token, "nonce": nonce }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let tokens = json_body(response).await;
    let access = tokens["access_token"].as_str().unwrap();
    let account = env
        .identity
        .resolve_access(access)
        .await
        .unwrap()
        .account_id;

    let rows = rows_of(&env, account).await;
    assert_eq!(actions(&rows), vec!["auth.sign_in"]);
    assert_eq!(rows[0]["actor_kind"], "user");
    assert_eq!(rows[0]["details"]["provider"], "apple");
    let text = rows[0].to_string();
    for secret in [
        access,
        tokens["refresh_token"].as_str().unwrap(),
        &email,
        &subject,
    ] {
        assert!(!text.contains(secret), "audit row leaks a secret: {text}");
    }
}

#[tokio::test]
async fn refresh_reuse_and_logout_all_are_audited() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, tokens) = env.signed_in_user().await;
    let refresh =
        |token: &str| post_json("/v1/auth/refresh", None, &json!({ "refresh_token": token }));
    let first = tokens.refresh_token.expose().to_owned();
    assert_eq!(
        send(&env.router, refresh(&first)).await.status(),
        StatusCode::OK
    );
    env.clock.advance(Duration::seconds(31));
    assert_eq!(
        send(&env.router, refresh(&first)).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let _second = env.identity.start_session(account, None).await.unwrap();
    env.identity
        .revoke_all_sessions(account, RevokeReason::LogoutAll)
        .await
        .unwrap();

    let rows = rows_of(&env, account).await;
    assert_eq!(
        actions(&rows),
        vec!["auth.refresh_reuse", "auth.logout_all"]
    );
    assert_eq!(
        rows[0]["target"],
        format!("session:{}", tokens.session_id.hyphenated())
    );
    assert_eq!(rows[1]["details"]["sessions"], 1);
}

#[tokio::test]
async fn age_refusal_is_audited_without_the_birth_date() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, session) = env.signed_in_user().await;
    let today = env.clock.now().date();
    let dob = Date::from_calendar_date(today.year() - 10, Month::March, 7)
        .unwrap()
        .to_string();
    let request = Request::post("/v1/me/age")
        .header("content-type", "application/json")
        .header(
            "authorization",
            format!("Bearer {}", session.access_token.expose()),
        )
        .body(Body::from(json!({ "birth_date": dob }).to_string()))
        .unwrap();
    let response = send(&env.router, request).await;
    assert_problem(response, StatusCode::FORBIDDEN, "AGE_GATE_REFUSED").await;

    let rows = rows_of(&env, account).await;
    assert_eq!(actions(&rows), vec!["auth.age_refused"]);
    let text = rows[0].to_string();
    assert!(!text.contains(&dob), "no DOB in the audit row: {text}");
    assert!(rows[0]["details"].get("age").is_none());
}

#[tokio::test]
async fn handle_claims_are_audited_without_the_handle() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, _) = env.signed_in_user().await;
    sqlx::query("UPDATE identity.accounts SET status = 'pending_profile' WHERE id = $1")
        .bind(account)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let session = env.identity.start_session(account, None).await.unwrap();
    let handle = format!("au{}", &Uuid::now_v7().simple().to_string()[20..]);
    let request = Request::put("/v1/me/handle")
        .header("content-type", "application/json")
        .header(
            "authorization",
            format!("Bearer {}", session.access_token.expose()),
        )
        .body(Body::from(json!({ "handle": handle }).to_string()))
        .unwrap();
    assert_eq!(send(&env.router, request).await.status(), StatusCode::OK);

    let rows = rows_of(&env, account).await;
    assert_eq!(actions(&rows), vec!["profile.handle_changed"]);
    assert_eq!(rows[0]["details"]["first_claim"], true);
    assert!(!rows[0].to_string().contains(&handle));
}

#[tokio::test]
async fn pii_details_are_refused_before_writing() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let actor = Uuid::now_v7();
    let entry = AuditEntry::new(Actor::User(actor), AuditAction::SignIn)
        .detail("email", "someone@example.com");
    assert!(
        record(env.identity.pool(), &entry, env.clock.now())
            .await
            .is_err()
    );
    assert!(rows_of(&env, actor).await.is_empty());
}
