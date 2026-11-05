//! T-24: GET /v1/sessions, DELETE /v1/sessions/{id}, POST /v1/sessions/revoke-all (AC-1.4,
//! SR-2), against real Postgres + Redis (skipped locally when unset, required in CI).

use crate::common::*;
use axum::body::Body;
use http::{Request, StatusCode};
use identity::IssuedSession;
use platform::clock::Clock;
use serde_json::json;
use time::Duration;
use uuid::Uuid;

/// A new active account with two device sessions.
async fn active_user(env: &TestEnv) -> (Uuid, IssuedSession, IssuedSession) {
    let (account, _) = env.signed_in_user().await;
    sqlx::query("UPDATE identity.accounts SET status = 'active' WHERE id = $1")
        .bind(account)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let phone = env.identity.start_session(account, None).await.unwrap();
    let tablet = env.identity.start_session(account, None).await.unwrap();
    // The onboarding session from sign-in is not needed.
    sqlx::query(
        "UPDATE identity.sessions SET revoked_at = now(), revoked_reason = 'logout'
         WHERE account_id = $1 AND scope = 'onboarding'",
    )
    .bind(account)
    .execute(env.identity.pool())
    .await
    .unwrap();
    (account, phone, tablet)
}

fn token(session: &IssuedSession) -> &str {
    session.access_token.expose()
}

fn delete(path: &str, token: &str, request_id: Uuid) -> Request<Body> {
    Request::delete(path)
        .header("authorization", format!("Bearer {token}"))
        .header("x-request-id", request_id.to_string())
        .body(Body::empty())
        .unwrap()
}

async fn me_status(env: &TestEnv, session: &IssuedSession) -> StatusCode {
    send(&env.router, get_request("/v1/me", Some(token(session))))
        .await
        .status()
}

#[tokio::test]
async fn list_shows_only_my_live_sessions_and_marks_the_current_one() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, phone, tablet) = active_user(&env).await;
    let (_, stranger, _) = active_user(&env).await;

    let response = send(
        &env.router,
        get_request("/v1/sessions", Some(token(&phone))),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let sessions = body["sessions"].as_array().unwrap();
    let ids: Vec<&str> = sessions.iter().map(|s| s["id"].as_str().unwrap()).collect();
    let (phone_id, tablet_id) = (
        phone.session_id.hyphenated().to_string(),
        tablet.session_id.hyphenated().to_string(),
    );
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&phone_id.as_str()) && ids.contains(&tablet_id.as_str()));
    assert!(
        !ids.contains(&stranger.session_id.hyphenated().to_string().as_str()),
        "never another account's sessions"
    );
    for session in sessions {
        let current = session["id"] == phone_id;
        assert_eq!(session["is_current"], current);
        assert!(session.get("access_token_hash").is_none());
    }
}

#[tokio::test]
async fn revoking_one_session_kills_its_tokens_and_keeps_mine() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, phone, tablet) = active_user(&env).await;
    let path = format!("/v1/sessions/{}", tablet.session_id);
    let response = send(&env.router, delete(&path, token(&phone), Uuid::now_v7())).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    assert_eq!(me_status(&env, &tablet).await, StatusCode::UNAUTHORIZED);
    let refresh = post_json(
        "/v1/auth/refresh",
        None,
        &json!({ "refresh_token": tablet.refresh_token.expose() }),
    );
    assert_problem(
        send(&env.router, refresh).await,
        StatusCode::UNAUTHORIZED,
        "SESSION_REVOKED",
    )
    .await;
    assert_eq!(me_status(&env, &phone).await, StatusCode::OK);

    // Already revoked: gone.
    let again = send(&env.router, delete(&path, token(&phone), Uuid::now_v7())).await;
    assert_problem(again, StatusCode::NOT_FOUND, "NOT_FOUND").await;

    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.audit_events
         WHERE actor_id = $1 AND action = 'auth.session_revoked' AND target = $2",
    )
    .bind(account)
    .bind(format!("session:{}", tablet.session_id))
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(audited, 1);
}

#[tokio::test]
async fn another_accounts_session_is_indistinguishable_from_a_missing_one() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, alice, _) = active_user(&env).await;
    let (_, bob, _) = active_user(&env).await;
    let request_id = Uuid::now_v7();

    let foreign = send(
        &env.router,
        delete(
            &format!("/v1/sessions/{}", alice.session_id),
            token(&bob),
            request_id,
        ),
    )
    .await;
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    let foreign = body_bytes(foreign).await;
    let missing = send(
        &env.router,
        delete(
            &format!("/v1/sessions/{}", Uuid::now_v7()),
            token(&bob),
            request_id,
        ),
    )
    .await;
    assert_eq!(body_bytes(missing).await, foreign, "byte-identical denial");
    let malformed = send(
        &env.router,
        delete("/v1/sessions/not-a-uuid", token(&bob), request_id),
    )
    .await;
    assert_eq!(body_bytes(malformed).await, foreign);

    assert_eq!(
        me_status(&env, &alice).await,
        StatusCode::OK,
        "Alice is untouched"
    );
}

#[tokio::test]
async fn log_out_everywhere_kills_every_token_and_widget_child_instantly() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, phone, tablet) = active_user(&env).await;
    // A widget child of the tablet session (the widget endpoint arrives with T-45).
    let now = env.clock.now();
    let widget_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO identity.sessions
           (id, account_id, access_token_hash, access_expires_at, scope, parent_session_id,
            idle_expires_at, absolute_expires_at, last_seen_at, created_at)
         VALUES ($1, $2, $3, $4, 'widget', $5, $4, $4, $6, $6)",
    )
    .bind(widget_id)
    .bind(account)
    .bind(Uuid::now_v7().as_bytes().repeat(2))
    .bind(now + Duration::days(30))
    .bind(tablet.session_id)
    .bind(now)
    .execute(env.identity.pool())
    .await
    .unwrap();
    // Warm the session cache, so the test proves eviction too.
    assert_eq!(me_status(&env, &tablet).await, StatusCode::OK);

    let request = Request::post("/v1/sessions/revoke-all")
        .header("authorization", format!("Bearer {}", token(&phone)))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send(&env.router, request).await.status(),
        StatusCode::NO_CONTENT
    );

    for session in [&phone, &tablet] {
        assert_eq!(me_status(&env, session).await, StatusCode::UNAUTHORIZED);
        let refresh = post_json(
            "/v1/auth/refresh",
            None,
            &json!({ "refresh_token": session.refresh_token.expose() }),
        );
        assert_eq!(
            send(&env.router, refresh).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.sessions WHERE account_id = $1 AND revoked_at IS NULL",
    )
    .bind(account)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(live, 0, "widget child revoked too");
}

#[tokio::test]
async fn onboarding_sessions_cannot_manage_sessions() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, onboarding) = env.signed_in_user().await;
    let response = send(
        &env.router,
        get_request("/v1/sessions", Some(token(&onboarding))),
    )
    .await;
    assert_problem(response, StatusCode::FORBIDDEN, "ONBOARDING_INCOMPLETE").await;
}
