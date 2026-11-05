//! T-43: exact-handle search without enumeration (US-5a AC-5a.1-5a.3, SR-19, SR-23, threat A-11)
//! against real Postgres + Redis, including the timing test (two-sample Kolmogorov-Smirnov, minor
//! vs missing handle, p > 0.05) and byte-identical misses.

use std::sync::Arc;
use std::time::Instant;

use crate::common::*;
use crate::social_support::*;
use axum::Router;
use axum::body::Body;
use http::{Request, StatusCode};
use platform::rate_limit::{RateLimitConfig, RateLimiter};
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

fn search_request(user: &User, handle: &str, request_id: Uuid) -> Request<Body> {
    Request::post("/v1/users/search")
        .header("authorization", format!("Bearer {}", user.token))
        .header("content-type", "application/json")
        .header("x-request-id", request_id.to_string())
        .body(Body::from(json!({ "handle": handle }).to_string()))
        .unwrap()
}

async fn search(router: &Router, user: &User, handle: &str) -> Value {
    let response = send(router, search_request(user, handle, Uuid::now_v7())).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

/// `POST /v1/friend-requests` with a search ref.
async fn request_via_ref(env: &TestEnv, from: &User, search_ref: &str) -> http::Response<Body> {
    let request = Request::post("/v1/friend-requests")
        .header("authorization", format!("Bearer {}", from.token))
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::now_v7().to_string())
        .body(Body::from(json!({ "search_ref": search_ref }).to_string()))
        .unwrap();
    send(&env.router, request).await
}

#[tokio::test]
async fn an_exact_case_insensitive_match_returns_one_result() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let searcher = established(&env, adult(&env).await).await;
    let target = adult(&env).await;
    for query in [
        target.handle.clone(),
        target.handle.to_uppercase(),
        format!("@{}", target.handle),
    ] {
        let body = search(&env.router, &searcher, &query).await;
        let result = body["result"].as_object().unwrap();
        let keys: Vec<&String> = result.keys().collect();
        assert_eq!(keys, ["display_name", "ref"], "no id, handle or age");
        assert_eq!(body["result"]["display_name"], target.name.as_str());
    }
}

#[tokio::test]
async fn partial_input_returns_nothing() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let searcher = established(&env, adult(&env).await).await;
    let target = adult(&env).await;
    let handle = &target.handle;
    for query in [
        handle[..handle.len() - 1].to_owned(),
        format!("{handle}x"),
        format!("{}%", &handle[..4]),
        format!("{}*", &handle[..4]),
        format!("{}.*", &handle[..4]),
        String::new(),
        "x".repeat(300),
    ] {
        assert_eq!(
            search(&env.router, &searcher, &query).await,
            json!({ "result": null }),
            "{query}"
        );
    }
}

/// Response bytes and headers (without the per-request ones) for one search.
async fn miss_fingerprint(
    router: &Router,
    user: &User,
    handle: &str,
    request_id: Uuid,
) -> (Vec<u8>, Vec<String>) {
    let response = send(router, search_request(user, handle, request_id)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut headers: Vec<String> = response
        .headers()
        .iter()
        .filter(|(name, _)| !matches!(name.as_str(), "x-request-id" | "date"))
        .map(|(name, value)| format!("{name}: {}", value.to_str().unwrap()))
        .collect();
    headers.sort();
    (body_bytes(response).await.to_vec(), headers)
}

#[tokio::test]
async fn hidden_accounts_answer_exactly_like_a_missing_handle() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let searcher = established(&env, adult(&env).await).await;
    let teen = minor(&env).await;
    let blocks_searcher = adult(&env).await;
    block(&env, blocks_searcher.id, searcher.id).await;
    let blocked_by_searcher = adult(&env).await;
    block(&env, searcher.id, blocked_by_searcher.id).await;
    let suspended = adult(&env).await;
    sqlx::query("UPDATE identity.accounts SET status = 'suspended' WHERE id = $1")
        .bind(suspended.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let request_id = Uuid::now_v7();
    let missing = miss_fingerprint(&env.router, &searcher, &fresh_handle(), request_id).await;
    assert_eq!(missing.0, br#"{"result":null}"#);
    for hidden in [
        &teen.handle,
        &blocks_searcher.handle,
        &blocked_by_searcher.handle,
        &suspended.handle,
        &searcher.handle,
    ] {
        let fingerprint = miss_fingerprint(&env.router, &searcher, hidden, request_id).await;
        assert_eq!(fingerprint, missing, "{hidden}");
    }
}

#[tokio::test]
async fn a_ref_works_once_for_its_searcher_only_and_expires() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = established(&env, adult(&env).await).await;
    let mallory = adult(&env).await;
    let target = adult(&env).await;
    let body = search(&env.router, &alice, &target.handle).await;
    let search_ref = body["result"]["ref"].as_str().unwrap().to_owned();

    // Minted for Alice: refused for anyone else, like a ref that never existed.
    let request_id = Uuid::now_v7();
    let stolen = send(&env.router, ref_request(&mallory, &search_ref, request_id)).await;
    assert_eq!(stolen.status(), StatusCode::NOT_FOUND);
    let forged = send(
        &env.router,
        ref_request(&mallory, "AAAAAAAAAAAAAAAAAAAAAAAAAAAA", request_id),
    )
    .await;
    assert_eq!(body_bytes(stolen).await, body_bytes(forged).await);

    let response = request_via_ref(&env, &alice, &search_ref).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = json_body(response).await["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(request_status(&env, &id).await, "pending");
    let source: String =
        sqlx::query_scalar("SELECT source FROM social.friend_requests WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(source, "search");

    // Ten minutes later a fresh ref is needed.
    let other = adult(&env).await;
    let late_ref = search(&env.router, &alice, &other.handle).await["result"]["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    env.clock
        .advance(Duration::minutes(10) + Duration::seconds(1));
    let alice = signed_in_again(&env, &alice).await;
    assert_problem(
        request_via_ref(&env, &alice, &late_ref).await,
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
    )
    .await;
}

fn ref_request(user: &User, search_ref: &str, request_id: Uuid) -> Request<Body> {
    Request::post("/v1/friend-requests")
        .header("authorization", format!("Bearer {}", user.token))
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::now_v7().to_string())
        .header("x-request-id", request_id.to_string())
        .body(Body::from(json!({ "search_ref": search_ref }).to_string()))
        .unwrap()
}

#[tokio::test]
async fn a_target_that_hides_after_the_search_gets_a_silent_drop() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let target = adult(&env).await;
    let search_ref = search(&env.router, &alice, &target.handle).await["result"]["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    block(&env, target.id, alice.id).await;
    let response = request_via_ref(&env, &alice, &search_ref).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = json_body(response).await["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(request_status(&env, &id).await, "dropped");
}

#[tokio::test]
async fn exactly_one_request_source_is_required() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    for body in [
        json!({}),
        json!({ "invite_token": "AAAAAAAAAAAAAAAAAAAAAA", "search_ref": "x" }),
    ] {
        let request = Request::post("/v1/friend-requests")
            .header("authorization", format!("Bearer {}", alice.token))
            .header("content-type", "application/json")
            .header("idempotency-key", Uuid::now_v7().to_string())
            .body(Body::from(body.to_string()))
            .unwrap();
        assert_problem(
            send(&env.router, request).await,
            StatusCode::BAD_REQUEST,
            "VALIDATION_FAILED",
        )
        .await;
    }
}

#[tokio::test]
async fn the_eleventh_search_in_a_minute_is_try_later() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let searcher = established(&env, adult(&env).await).await;
    for _ in 0..10 {
        search(&env.router, &searcher, &fresh_handle()).await;
    }
    let response = send(
        &env.router,
        search_request(&searcher, &fresh_handle(), Uuid::now_v7()),
    )
    .await;
    assert_problem(response, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await;
}

/// The production limits with the search limits lifted (the timing test sends 2,000 searches).
fn limits_without_search_caps() -> RateLimitConfig {
    let mut yaml: serde_norway::Value = serde_norway::from_str(LIMITS_YAML).unwrap();
    for name in ["users.search", "users.search.device", "users.search.ip"] {
        yaml["limits"][name] = serde_norway::from_str(
            "{ scope: user, requests: 1000000, per_secs: 60, on_store_failure: open }",
        )
        .unwrap();
    }
    RateLimitConfig::from_yaml_str(&serde_norway::to_string(&yaml).unwrap()).unwrap()
}

/// Two-sample Kolmogorov-Smirnov test: the asymptotic p-value of the largest gap between the two
/// empirical distribution functions.
fn ks_p_value(a: &[f64], b: &[f64]) -> f64 {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    a.sort_by(f64::total_cmp);
    b.sort_by(f64::total_cmp);
    let (n, m) = (a.len() as f64, b.len() as f64);
    let (mut i, mut j, mut d) = (0, 0, 0.0f64);
    while i < a.len() && j < b.len() {
        let x = a[i].min(b[j]);
        while i < a.len() && a[i] <= x {
            i += 1;
        }
        while j < b.len() && b[j] <= x {
            j += 1;
        }
        d = d.max((i as f64 / n - j as f64 / m).abs());
    }
    let en = (n * m / (n + m)).sqrt();
    let lambda = (en + 0.12 + 0.11 / en) * d;
    // The series does not converge near 0, where Q_KS is 1 to many decimals.
    if lambda < 0.3 {
        return 1.0;
    }
    // Q_KS(lambda) = 2 * sum (-1)^(k-1) exp(-2 k^2 lambda^2)
    let mut sum = 0.0;
    for k in 1..=100 {
        let k = f64::from(k);
        let sign = if k as i64 % 2 == 1 { 1.0 } else { -1.0 };
        sum += sign * (-2.0 * k * k * lambda * lambda).exp();
    }
    (2.0 * sum).clamp(0.0, 1.0)
}

#[test]
fn ks_test_tells_different_samples_apart() {
    let same: Vec<f64> = (0..500).map(|i| f64::from(i % 50)).collect();
    assert!(ks_p_value(&same, &same) > 0.99);
    let shifted: Vec<f64> = same.iter().map(|x| x + 10.0).collect();
    assert!(ks_p_value(&same, &shifted) < 0.001);
}

/// Wall time of one search, in whole milliseconds (finer than any network observer).
async fn timed_search(
    router: &Router,
    user: &User,
    handle: &str,
    request_id: Uuid,
) -> (f64, Vec<u8>) {
    let started = Instant::now();
    let response = send(router, search_request(user, handle, request_id)).await;
    let bytes = body_bytes(response).await.to_vec();
    ((started.elapsed().as_secs_f64() * 1000.0).round(), bytes)
}

#[tokio::test]
async fn minor_and_missing_handles_cannot_be_told_apart_by_timing_or_bytes() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    const SAMPLES: usize = 1_000;
    let mut settings = social_settings();
    settings.search_latency_floor_ms = 40;
    let mut state = env.state.clone().with_social_settings(settings);
    state.limiter = Arc::new(RateLimiter::in_memory(
        limits_without_search_caps(),
        Arc::new(platform::clock::SystemClock),
    ));
    let router = api::build(api::openapi::router(), state, &test_config());

    let searcher = established(&env, adult(&env).await).await;
    let teen = minor(&env).await;
    let request_id = Uuid::now_v7();
    // Warm caches and connections on both paths first.
    for _ in 0..20 {
        timed_search(&router, &searcher, &teen.handle, request_id).await;
        timed_search(&router, &searcher, &fresh_handle(), request_id).await;
    }
    // One retry: under identical distributions p <= 0.05 still happens 5% of the time.
    let mut last_p = 0.0;
    for _attempt in 0..2 {
        let mut minor_ms = Vec::with_capacity(SAMPLES);
        let mut missing_ms = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let (ms, minor_bytes) =
                timed_search(&router, &searcher, &teen.handle, request_id).await;
            minor_ms.push(ms);
            let (ms, missing_bytes) =
                timed_search(&router, &searcher, &fresh_handle(), request_id).await;
            missing_ms.push(ms);
            assert_eq!(minor_bytes, missing_bytes, "identical bytes");
            assert_eq!(minor_bytes, br#"{"result":null}"#);
        }
        last_p = ks_p_value(&minor_ms, &missing_ms);
        if last_p > 0.05 {
            break;
        }
    }
    assert!(last_p > 0.05, "KS p = {last_p}");
}
