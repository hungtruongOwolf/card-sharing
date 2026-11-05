//! T-13: layered rate limiter through the HTTP stack, with and without Redis.

use std::net::IpAddr;
use std::sync::Arc;

use crate::common::*;
use api::access::{ApiRouter, RoutePolicy};
use api::state::AppState;
use axum::Router;
use axum::body::Body;
use http::{Request, StatusCode};
use platform::clock::{Clock, ManualClock, SystemClock};
use platform::config::Secret;
use platform::rate_limit::{AccountSubject, Decision, RateLimiter, Subject};
use time::OffsetDateTime;
use utoipa_axum::routes;
use uuid::Uuid;

/// Public route under the fail-closed `auth.signin` limit (10/min per IP).
#[utoipa::path(post, path = "/test/signin", responses((status = OK, description = "ok")))]
async fn signin_route() -> &'static str {
    "ok"
}

/// Public route under the fail-open `config.read` limit.
#[utoipa::path(get, path = "/test/config", responses((status = OK, description = "ok")))]
async fn config_route() -> &'static str {
    "ok"
}

fn routes() -> ApiRouter<AppState> {
    api::openapi::router()
        .route(
            RoutePolicy::public().limited("auth.signin"),
            routes!(signin_route),
        )
        .route(
            RoutePolicy::public().limited("config.read"),
            routes!(config_route),
        )
}

fn app(limiter: RateLimiter) -> Router {
    let mut config = test_config();
    config.client_ip_header = Some(http::HeaderName::from_static("x-test-client-ip"));
    api::build(
        routes(),
        AppState::new(
            policy_service(),
            limiter,
            offline_identity(),
            client_config(),
        ),
        &config,
    )
}

fn signin_from(ip: &str) -> Request<Body> {
    Request::post("/test/signin")
        .header("x-test-client-ip", ip)
        .body(Body::empty())
        .unwrap()
}

/// A limiter pointed at a closed port: every Redis call fails.
fn redis_down_limiter() -> RateLimiter {
    let redis = platform::cache::connect_lazy(&Secret::new("redis://127.0.0.1:1/0")).unwrap();
    RateLimiter::with_redis(limits_config(), redis, Arc::new(SystemClock))
}

#[test]
fn shipped_limits_config_covers_every_route() {
    let (_, _, policies) = api::openapi::router().finish();
    limits_config()
        .ensure_defined(policies.rate_limits())
        .unwrap();
}

#[tokio::test]
async fn eleventh_signin_per_minute_from_one_ip_is_429_with_retry_after() {
    let router = app(memory_limiter());
    for _ in 0..10 {
        assert_eq!(
            send(&router, signin_from("203.0.113.7")).await.status(),
            StatusCode::OK
        );
    }
    let limited = send(&router, signin_from("203.0.113.7")).await;
    let retry_after: u64 = limited
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .expect("Retry-After header");
    assert!((1..=60).contains(&retry_after));
    assert_problem(limited, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await;

    // Another IP has its own bucket.
    assert_eq!(
        send(&router, signin_from("203.0.113.8")).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn redis_down_fails_closed_for_auth_and_open_for_others() {
    let router = app(redis_down_limiter());
    let response = send(&router, signin_from("203.0.113.9")).await;
    assert!(response.headers().get("retry-after").is_some());
    assert_problem(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "TEMPORARILY_UNAVAILABLE",
    )
    .await;

    let response = send(&router, get_request("/test/config", None)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "fail-open limit falls back to memory"
    );
}

// ---- Against a real Redis/Valkey (TEST_REDIS_URL) -------------------------------------------

fn redis_limiter(clock: Arc<ManualClock>) -> Option<RateLimiter> {
    let url = service_url("TEST_REDIS_URL")?;
    let redis = platform::cache::connect_lazy(&Secret::new(url)).unwrap();
    Some(
        RateLimiter::with_redis(limits_config(), redis, clock)
            .with_key_prefix(format!("test-rl-{}", Uuid::now_v7())),
    )
}

fn account(created_at: OffsetDateTime) -> Option<AccountSubject> {
    Some(AccountSubject {
        id: Uuid::now_v7(),
        created_at,
    })
}

#[tokio::test]
async fn redis_same_account_on_five_devices_shares_one_quota() {
    let clock = Arc::new(ManualClock::starting_now());
    let Some(limiter) = redis_limiter(clock.clone()) else {
        return;
    };
    let account = account(OffsetDateTime::UNIX_EPOCH);
    // me.read: 120/min per user. Spread 121 calls over 5 devices and IPs.
    let mut allowed = 0;
    for n in 0..121u32 {
        let subject = Subject {
            account,
            device: Some(Uuid::now_v7()),
            ip: Some(IpAddr::from([10, 1, 0, u8::try_from(n % 5).unwrap()])),
        };
        if limiter.check("me.read", &subject).await.unwrap() == Decision::Allowed {
            allowed += 1;
        }
    }
    assert_eq!(allowed, 120);
}

#[tokio::test]
async fn redis_bucket_refills_with_the_clock() {
    let clock = Arc::new(ManualClock::starting_now());
    let Some(limiter) = redis_limiter(clock.clone()) else {
        return;
    };
    let subject = Subject {
        ip: Some(IpAddr::from([10, 2, 0, 1])),
        ..Subject::default()
    };
    for _ in 0..10 {
        assert_eq!(
            limiter.check("auth.signin", &subject).await.unwrap(),
            Decision::Allowed
        );
    }
    assert_eq!(
        limiter.check("auth.signin", &subject).await.unwrap(),
        Decision::Limited {
            retry_after_secs: 6
        }
    );
    clock.advance(time::Duration::seconds(6));
    assert_eq!(
        limiter.check("auth.signin", &subject).await.unwrap(),
        Decision::Allowed
    );
}

#[tokio::test]
async fn redis_new_account_quota_applies_for_seven_days_then_lifts() {
    let clock = Arc::new(ManualClock::starting_now());
    let Some(url) = service_url("TEST_REDIS_URL") else {
        return;
    };
    let yaml = r"
version: 1
new_account: { days: 7 }
limits:
  test.new:
    scope: user
    requests: 4
    per_secs: 3600
    on_store_failure: open
    new_account_requests: 2
";
    let config = platform::rate_limit::RateLimitConfig::from_yaml_str(yaml).unwrap();
    let redis = platform::cache::connect_lazy(&Secret::new(url)).unwrap();
    let limiter = RateLimiter::with_redis(config, redis, clock.clone())
        .with_key_prefix(format!("test-rl-{}", Uuid::now_v7()));
    let subject = Subject {
        account: account(clock.now()),
        ..Subject::default()
    };
    let mut allowed = 0;
    for _ in 0..4 {
        if limiter.check("test.new", &subject).await.unwrap() == Decision::Allowed {
            allowed += 1;
        }
    }
    assert_eq!(allowed, 2, "new account gets the reduced quota");

    clock.advance(time::Duration::days(7));
    let mut allowed = 0;
    for _ in 0..5 {
        if limiter.check("test.new", &subject).await.unwrap() == Decision::Allowed {
            allowed += 1;
        }
    }
    assert_eq!(allowed, 4, "full quota after 7 days");
}
