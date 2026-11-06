//! T-27: GET /v1/me, GET /v1/config and the minimum app version gate.

use crate::common::*;
use axum::body::Body;
use http::{Request, StatusCode};

fn get_with(path: &str, token: Option<&str>, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::get(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn onboarding_session_sees_its_own_account_at_the_age_step() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, tokens) = env.signed_in_user().await;
    let response = send(
        &env.router,
        get_request("/v1/me", Some(tokens.access_token.expose())),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["account"]["id"], account.to_string());
    assert_eq!(body["account"]["status"], "pending_age");
    assert_eq!(body["onboarding"]["step"], "age");
    assert_eq!(
        body["policy"]["min_age"], 13,
        "default policy without signals"
    );
}

#[tokio::test]
async fn me_never_returns_another_users_data() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account_a, tokens_a) = env.signed_in_user().await;
    let (account_b, tokens_b) = env.signed_in_user().await;
    for (account, token) in [(account_a, &tokens_a), (account_b, &tokens_b)] {
        let body = json_body(
            send(
                &env.router,
                get_request("/v1/me", Some(token.access_token.expose())),
            )
            .await,
        )
        .await;
        assert_eq!(body["account"]["id"], account.to_string());
    }
    // Query strings cannot redirect the lookup either.
    let path = format!("/v1/me?account_id={account_b}");
    let body = json_body(
        send(
            &env.router,
            get_request(&path, Some(tokens_a.access_token.expose())),
        )
        .await,
    )
    .await;
    assert_eq!(body["account"]["id"], account_a.to_string());
}

#[tokio::test]
async fn me_uses_the_strictest_policy_including_the_current_ip_country() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, tokens) = env.signed_in_user().await;
    let request = get_with(
        "/v1/me",
        Some(tokens.access_token.expose()),
        &[(TEST_COUNTRY_HEADER, "DE")],
    );
    let body = json_body(send(&env.router, request).await).await;
    assert_eq!(body["policy"]["min_age"], 16);
    assert_eq!(body["policy"]["ads"]["personalized_requires_consent"], true);
}

#[tokio::test]
async fn me_requires_authentication() {
    let response = send(&offline_app(), get_request("/v1/me", None)).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}

#[tokio::test]
async fn config_is_public_cacheable_and_free_of_user_data() {
    let router = offline_app();
    let anonymous = send(&router, get_request("/v1/config", None)).await;
    assert_eq!(anonymous.status(), StatusCode::OK);
    assert_eq!(
        anonymous
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("public, max-age=300")
    );
    let anonymous_body = body_bytes(anonymous).await;

    // A bearer token changes nothing: same bytes, no account data.
    let with_token = send(&router, get_request("/v1/config", Some("some-token"))).await;
    assert_eq!(body_bytes(with_token).await, anonymous_body);

    let body: serde_json::Value = serde_json::from_slice(&anonymous_body).unwrap();
    let keys: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "flags",
            "kill_switches",
            "limits",
            "min_app_version",
            "reactions",
            "text_filter"
        ]
    );
}

#[tokio::test]
async fn config_serves_the_text_filter_hints_with_an_etag() {
    let router = offline_app();
    let response = send(&router, get_request("/v1/config", None)).await;
    let etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_owned();
    assert!(etag.starts_with('"') && etag.ends_with('"'));
    let body = json_body(response).await;
    // Exactly the lists of config/text_filter.yaml, as written (#213).
    let yaml: serde_json::Value = serde_norway::from_str(TEXT_FILTER_YAML).unwrap();
    assert_eq!(body["text_filter"], yaml);

    // A current If-None-Match answers 304 with no body; a stale one gets the full body.
    let request = Request::get("/v1/config")
        .header("if-none-match", &etag)
        .body(Body::empty())
        .unwrap();
    let cached = send(&router, request).await;
    assert_eq!(cached.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        cached.headers().get("etag").and_then(|v| v.to_str().ok()),
        Some(etag.as_str())
    );
    assert!(body_bytes(cached).await.is_empty());
    let request = Request::get("/v1/config")
        .header("if-none-match", "\"stale\"")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&router, request).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn outdated_app_version_gets_426_except_on_config() {
    let router = offline_app();
    let old = [("x-app-version", "0.0.9")];
    let response = send(&router, get_with("/v1/me", None, &old)).await;
    assert_problem(response, StatusCode::UPGRADE_REQUIRED, "UPGRADE_REQUIRED").await;

    // The app must still be able to read the minimum version.
    let config = send(&router, get_with("/v1/config", None, &old)).await;
    assert_eq!(config.status(), StatusCode::OK);
    let health = send(&router, get_with("/healthz", None, &old)).await;
    assert_eq!(health.status(), StatusCode::OK);
}

#[tokio::test]
async fn current_missing_or_malformed_app_versions() {
    let router = offline_app();
    // Current or newer version, or no header (web, tools): the gate passes, auth decides.
    for headers in [
        vec![("x-app-version", "0.1.0")],
        vec![("x-app-version", "2.3.4")],
        vec![],
    ] {
        let response = send(&router, get_with("/v1/me", None, &headers)).await;
        assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
    }
    let response = send(
        &router,
        get_with("/v1/me", None, &[("x-app-version", "latest")]),
    )
    .await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
}
