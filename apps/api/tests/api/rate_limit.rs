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

/// Public route under `auth.refresh` (30/h per device, falling back to IP; fails open).
#[utoipa::path(post, path = "/test/refresh", responses((status = OK, description = "ok")))]
async fn refresh_route() -> &'static str {
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
        .route(
            RoutePolicy::public().limited("auth.refresh"),
            routes!(refresh_route),
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
            field_cipher(),
            profile_settings(),
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

/// Fail-closed limits refuse every request while the store is slow or down, so each one is a
/// reviewed security decision (sprint 9, security-baseline "Rate limiter failure mode"). Only
/// these kinds of routes may fail closed: sign-in and credential steps (brute force, token
/// stuffing), guessable tokens (invites, public links, export downloads), enumeration (handle
/// check, search), staff routes, and abuse-prone or irreversible writes (friend requests, guest
/// reports, deletion and export requests, the age gate). Everything else fails open to the
/// per-instance memory bucket. Adding a limit here needs a security review.
#[test]
fn fail_closed_limits_are_the_reviewed_set() {
    let reviewed = [
        "admin.actions",
        "admin.auth.login",
        "admin.auth.mfa",
        "admin.read",
        "attestation.challenge",
        "attestation.register",
        "auth.dev",
        "auth.email.start",
        "auth.email.start.email",
        "auth.email.verify",
        "auth.reauth",
        "auth.reauth.email.start",
        "auth.signin",
        "friend_requests.create",
        "invites.preview",
        "me.age",
        "me.handle.check",
        "me.handle.claim",
        "privacy.deletion.cancel",
        "privacy.deletion.request",
        "privacy.exports.download",
        "privacy.exports.request",
        "public.boards.read",
        "public.invites.read",
        "public.reports.create",
        "users.search",
        "users.search.device",
        "users.search.ip",
    ];
    assert_eq!(limits_config().fail_closed_limits(), reviewed);
}

#[tokio::test]
async fn redis_down_fails_closed_for_auth_and_open_for_others() {
    let router = app(redis_down_limiter());
    let response = send(&router, signin_from("203.0.113.9")).await;
    assert_eq!(
        response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok()),
        Some("2"),
        "Retry-After is one breaker window"
    );
    // A distinct code, so the app says "server busy" instead of "can't reach the server".
    assert_problem(response, StatusCode::SERVICE_UNAVAILABLE, "SERVER_BUSY").await;

    let response = send(&router, get_request("/test/config", None)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "fail-open limit falls back to memory"
    );
}

/// EM decision 2026-11-07: token refresh keeps working while the store is down, still limited by
/// the per-instance memory bucket (30 per hour per device/IP), never unlimited.
#[tokio::test]
async fn refresh_fails_open_to_the_memory_bucket_when_the_store_is_down() {
    let router = app(redis_down_limiter());
    let refresh_from = |ip: &str| {
        Request::post("/test/refresh")
            .header("x-test-client-ip", ip)
            .body(Body::empty())
            .unwrap()
    };
    for _ in 0..30 {
        assert_eq!(
            send(&router, refresh_from("203.0.113.20")).await.status(),
            StatusCode::OK
        );
    }
    let limited = send(&router, refresh_from("203.0.113.20")).await;
    assert!(limited.headers().contains_key("retry-after"));
    assert_problem(limited, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await;
    assert_eq!(
        send(&router, refresh_from("203.0.113.21")).await.status(),
        StatusCode::OK
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
