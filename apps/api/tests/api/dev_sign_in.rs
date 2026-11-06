//! #306: development-only sign-in for the seed users. Absent (uniform 404) unless
//! `API_DEV_SIGN_IN` is on in a development or test profile; signs into seed personas only.

use crate::common::*;
use crate::media::without_request_id;
use api::config::{AppEnv, Config};
use axum::Router;
use http::StatusCode;
use identity::{Provider, VerifiedIdentity};
use serde_json::json;
use uuid::Uuid;

fn config(env: AppEnv, dev_sign_in: bool) -> Config {
    let mut config = test_config();
    config.content_safety.env = env;
    config.dev_sign_in = dev_sign_in;
    config
}

async fn not_found_like_unknown_path(router: &Router) {
    let unknown = without_request_id(
        body_bytes(send(router, get_request("/v1/dev/does-not-exist", None)).await).await,
    );
    for request in [
        get_request("/v1/dev/seed-users", None),
        post_json("/v1/dev/sign-in", None, &json!({"handle": "alice.demo"})),
    ] {
        let response = send(router, request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(without_request_id(body_bytes(response).await), unknown);
    }
}

#[tokio::test]
async fn routes_are_absent_unless_enabled_in_development_or_test() {
    // Default test config: flag off.
    not_found_like_unknown_path(&offline_app()).await;
    // A hand-built config cannot enable it in staging or production either.
    for env in [AppEnv::Staging, AppEnv::Production] {
        let router = api::app(offline_state(), &config(env, true));
        not_found_like_unknown_path(&router).await;
    }
}

#[tokio::test]
async fn openapi_documents_the_dev_routes_for_the_client() {
    let spec = api::openapi::router().finish().1;
    let paths = &spec.paths.paths;
    assert!(paths.contains_key("/v1/dev/seed-users"));
    assert!(paths.contains_key("/v1/dev/sign-in"));
}

async fn seed_account(env: &TestEnv, key: &str) -> Uuid {
    let identity = VerifiedIdentity {
        provider: Provider::Apple,
        subject: api::seed::seed_subject(key),
        email: None,
    };
    env.identity
        .find_or_create_account(&identity, &Default::default())
        .await
        .unwrap()
        .0
}

#[tokio::test]
async fn enabled_it_signs_into_seed_accounts_only() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let router = api::build(
        api::openapi::router(),
        env.state.clone(),
        &config(AppEnv::Test, true),
    );
    let alice = seed_account(&env, "alice").await;

    let listed = json_body(send(&router, get_request("/v1/dev/seed-users", None)).await).await;
    let users = listed["users"].as_array().unwrap();
    let entry = users
        .iter()
        .find(|u| u["handle"] == "alice.demo")
        .expect("alice is listed");
    assert_eq!(entry["minor"], false);
    assert_eq!(entry["display_name"], "Alice (demo)");

    let response = send(
        &router,
        post_json(
            "/v1/dev/sign-in",
            None,
            &json!({
                "handle": "alice.demo",
                "device": {"id": Uuid::now_v7(), "platform": "ios", "app_version": "1.0.0"},
            }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let pair = json_body(response).await;
    assert_eq!(pair["token_type"], "Bearer");
    let access = pair["access_token"].as_str().unwrap();
    let me = send(&router, get_request("/v1/me", Some(access))).await;
    assert_eq!(me.status(), StatusCode::OK);
    assert_eq!(json_body(me).await["account"]["id"], alice.to_string());

    // A normal session: the refresh token rotates.
    let refreshed = send(
        &router,
        post_json(
            "/v1/auth/refresh",
            None,
            &json!({"refresh_token": pair["refresh_token"]}),
        ),
    )
    .await;
    assert_eq!(refreshed.status(), StatusCode::OK);

    // Audited as a sign-in with provider `dev`.
    let provider: Option<String> = sqlx::query_scalar(
        "SELECT details->>'provider' FROM platform.audit_events
         WHERE actor_id = $1 AND action = 'auth.sign_in' ORDER BY at DESC LIMIT 1",
    )
    .bind(alice)
    .fetch_optional(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(provider.as_deref(), Some("dev"));

    // Anything that is not a seed persona's handle is not found; extra fields are refused.
    for handle in ["eve.demo", "alice", "", "seed.alice"] {
        let response = send(
            &router,
            post_json("/v1/dev/sign-in", None, &json!({"handle": handle})),
        )
        .await;
        assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    }
    let response = send(
        &router,
        post_json(
            "/v1/dev/sign-in",
            None,
            &json!({"handle": "alice.demo", "account_id": alice}),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
