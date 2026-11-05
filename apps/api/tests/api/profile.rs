//! T-31: PUT /v1/me/profile (US-3, AC-3.2, SR-28) against real Postgres + Redis.

use crate::common::*;
use axum::body::Body;
use http::{Request, StatusCode};
use identity::IssuedSession;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn put_json(path: &str, token: &str, body: &Value) -> Request<Body> {
    Request::put(path)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// A signed-in account that passed the age gate (status set before its first request, so no
/// cached session holds the old status).
pub async fn profile_step_user(env: &TestEnv) -> (Uuid, IssuedSession) {
    let (account, tokens) = env.signed_in_user().await;
    sqlx::query("UPDATE identity.accounts SET status = 'pending_profile' WHERE id = $1")
        .bind(account)
        .execute(env.identity.pool())
        .await
        .unwrap();
    (account, tokens)
}

async fn put_name(env: &TestEnv, tokens: &IssuedSession, body: &Value) -> http::Response<Body> {
    send(
        &env.router,
        put_json("/v1/me/profile", tokens.access_token.expose(), body),
    )
    .await
}

async fn stored_name(env: &TestEnv, account: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT display_name FROM identity.profiles WHERE account_id = $1")
        .bind(account)
        .fetch_optional(env.identity.pool())
        .await
        .unwrap()
        .flatten()
}

#[tokio::test]
async fn sets_and_returns_the_display_name() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, tokens) = profile_step_user(&env).await;
    let response = put_name(&env, &tokens, &json!({"display_name": "Mai"})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["profile"]["display_name"], "Mai");
    // A name alone does not finish onboarding.
    assert_eq!(body["status"], "pending_profile");
    assert_eq!(body["onboarding"]["step"], "profile");
    assert_eq!(stored_name(&env, account).await.as_deref(), Some("Mai"));

    // Changing it is allowed (not unique, no cooldown) and GET /v1/me shows it.
    let response = put_name(&env, &tokens, &json!({"display_name": "Mai the cat"})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let me = json_body(
        send(
            &env.router,
            get_request("/v1/me", Some(tokens.access_token.expose())),
        )
        .await,
    )
    .await;
    assert_eq!(me["profile"]["display_name"], "Mai the cat");
}

#[tokio::test]
async fn display_names_are_not_unique() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    for _ in 0..2 {
        let (_, tokens) = profile_step_user(&env).await;
        let response = put_name(&env, &tokens, &json!({"display_name": "Biscuit"})).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn names_are_sanitized_before_counting_and_storing() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, tokens) = profile_step_user(&env).await;
    // Bidi override, zero-width space and padding are removed; decomposed accents composed.
    let raw = "  Jose\u{0301}\u{202E} \u{200B}  Cat ";
    let body = json_body(put_name(&env, &tokens, &json!({"display_name": raw})).await).await;
    assert_eq!(body["profile"]["display_name"], "Jos\u{00E9} Cat");
    assert_eq!(
        stored_name(&env, account).await.as_deref(),
        Some("Jos\u{00E9} Cat")
    );

    // Zalgo is capped, not rejected: two visible characters remain.
    let zalgo = format!("Z{}a", "\u{0336}\u{0301}".repeat(20));
    let body = json_body(put_name(&env, &tokens, &json!({"display_name": zalgo})).await).await;
    let stored = body["profile"]["display_name"].as_str().unwrap().to_owned();
    assert!(stored.chars().count() <= 2 + 4, "{stored:?}");
}

#[tokio::test]
async fn ac_3_2_validation_cases() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, tokens) = profile_step_user(&env).await;
    let thirty_cats = "\u{1F431}".repeat(30);
    let ok = [
        "Mi",
        thirty_cats.as_str(),
        "Nguy\u{1EC5}n V\u{0103}n An",
        "Scunthorpe",
    ];
    for name in ok {
        let response = put_name(&env, &tokens, &json!({"display_name": name})).await;
        assert_eq!(response.status(), StatusCode::OK, "{name}");
    }
    // Built at runtime so the source holds no literal term.
    let blocked: String = ['f', 'u', 'c', 'k'].iter().collect();
    let invisible_only = "\u{3164}\u{200B}\u{3164}";
    let bad = [
        "M".to_owned(),
        "  M  ".to_owned(),
        invisible_only.to_owned(),
        "a".repeat(31),
        format!("mai {blocked}"),
        format!("m.a.i {}", blocked.to_uppercase()),
    ];
    for name in bad {
        let response = put_name(&env, &tokens, &json!({"display_name": name})).await;
        let problem = assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
        let detail = problem["detail"].as_str().unwrap_or_default();
        assert!(!detail.to_lowercase().contains(&blocked), "{detail}");
    }
    for body in [
        json!({}),
        json!({"display_name": 42}),
        json!({"display_name": null}),
    ] {
        let response = put_name(&env, &tokens, &body).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    // The last accepted name is still stored.
    assert_eq!(
        stored_name(&env, account).await.as_deref(),
        Some("Scunthorpe")
    );
}

#[tokio::test]
async fn cannot_set_another_users_profile() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (victim, victim_tokens) = profile_step_user(&env).await;
    let (attacker, attacker_tokens) = profile_step_user(&env).await;
    let response = put_name(&env, &victim_tokens, &json!({"display_name": "Victim"})).await;
    assert_eq!(response.status(), StatusCode::OK);

    // No account id is accepted in the body, in any spelling.
    for field in ["account_id", "user_id", "id"] {
        let body = json!({"display_name": "Pwned", field: victim.to_string()});
        let response = put_name(&env, &attacker_tokens, &body).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    // Query strings are ignored: the write lands on the caller's own profile.
    let path = format!("/v1/me/profile?account_id={victim}");
    let response = send(
        &env.router,
        put_json(
            &path,
            attacker_tokens.access_token.expose(),
            &json!({"display_name": "Pwned"}),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(stored_name(&env, victim).await.as_deref(), Some("Victim"));
    assert_eq!(stored_name(&env, attacker).await.as_deref(), Some("Pwned"));
}

#[tokio::test]
async fn age_gate_must_be_passed_first() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, tokens) = env.signed_in_user().await;
    let response = put_name(&env, &tokens, &json!({"display_name": "Mai"})).await;
    assert_problem(response, StatusCode::FORBIDDEN, "ONBOARDING_INCOMPLETE").await;
    assert_eq!(stored_name(&env, account).await, None);
}

#[tokio::test]
async fn profile_requires_authentication() {
    let request = Request::put("/v1/me/profile")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"display_name":"Mai"}"#))
        .unwrap();
    let response = send(&offline_app(), request).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}
