//! T-41: friend-request caps (20 open, 30 per 24 hours) and the 14-day expiry (US-5 AC-5.3,
//! AC-5.4, SR-22) against real Postgres + Redis.

use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use platform::clock::Clock;
use serde_json::Value;
use social::SocialJanitor;
use time::Duration;
use uuid::Uuid;

/// Inserts `count` requests from `from` to fresh accounts with `status`, created `age` ago.
async fn seed_requests(env: &TestEnv, from: &User, count: usize, status: &str, age: Duration) {
    let created = env.clock.now() - age;
    for _ in 0..count {
        let (to, _) = env.signed_in_user().await;
        sqlx::query(
            "INSERT INTO social.friend_requests (id, from_user, to_user, source, status, created_at, expires_at)
             VALUES ($1, $2, $3, 'invite', $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(from.id)
        .bind(to)
        .bind(status)
        .bind(created)
        .bind(created + Duration::days(14))
        .execute(env.identity.pool())
        .await
        .unwrap();
    }
}

async fn list(env: &TestEnv, user: &User, direction: &str) -> Vec<Value> {
    let path = format!("/v1/friend-requests?direction={direction}");
    let response = call(env, "GET", &path, user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["items"]
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn the_21st_open_request_is_rate_limited() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sender = adult(&env).await;
    // Declined and dropped requests still count: they look pending to the sender.
    seed_requests(&env, &sender, 10, "pending", Duration::days(3)).await;
    seed_requests(&env, &sender, 5, "declined", Duration::days(3)).await;
    seed_requests(&env, &sender, 5, "dropped", Duration::days(3)).await;
    let target = adult(&env).await;
    let token = invite_token(&env, &target).await;
    let response = request_via_invite(&env, &sender, &token).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after: i64 = response.headers()["retry-after"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    // The oldest open request expires in 11 days.
    assert!((retry_after - 11 * 86_400).abs() < 5, "{retry_after}");
    assert_problem(response, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await;
}

#[tokio::test]
async fn the_31st_request_in_24_hours_is_rate_limited() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sender = adult(&env).await;
    // Cancelled ones count too (no cancel-and-resend loop).
    seed_requests(&env, &sender, 30, "cancelled", Duration::hours(2)).await;
    let target = adult(&env).await;
    let token = invite_token(&env, &target).await;
    assert_problem(
        request_via_invite(&env, &sender, &token).await,
        StatusCode::TOO_MANY_REQUESTS,
        "RATE_LIMITED",
    )
    .await;
    // A day later the window has moved on.
    env.clock.advance(Duration::hours(23));
    let sender = signed_in_again(&env, &sender).await;
    let target = signed_in_again(&env, &target).await;
    let token = invite_token(&env, &target).await;
    let response = request_via_invite(&env, &sender, &token).await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn caps_apply_even_when_the_target_blocked_the_sender() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sender = adult(&env).await;
    seed_requests(&env, &sender, 20, "pending", Duration::days(1)).await;
    let blocker = adult(&env).await;
    block(&env, blocker.id, sender.id).await;
    let token = invite_token(&env, &blocker).await;
    // Same answer as for anyone else: a block is not revealed by a cap that stops applying.
    assert_problem(
        request_via_invite(&env, &sender, &token).await,
        StatusCode::TOO_MANY_REQUESTS,
        "RATE_LIMITED",
    )
    .await;
}

// The janitor runs over the whole (shared) test database, so these tests never move a clock
// forward before running it: they age their own rows instead, and the janitor uses the present.

fn janitor(env: &TestEnv) -> SocialJanitor {
    SocialJanitor::new(
        env.identity.pool().clone(),
        env.clock.clone(),
        social_settings().retention_days,
    )
}

#[tokio::test]
async fn a_request_older_than_14_days_disappears_from_both_lists() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let token = invite_token(&env, &alice).await;
    let response = request_via_invite(&env, &bob, &token).await;
    let id = json_body(response).await["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(list(&env, &alice, "incoming").await.len(), 1);

    // Sent 14 days and a second ago.
    let now = env.clock.now();
    sqlx::query(
        "UPDATE social.friend_requests SET created_at = $2, expires_at = $3 WHERE id = $1::uuid",
    )
    .bind(&id)
    .bind(now - Duration::days(14) - Duration::seconds(1))
    .bind(now - Duration::seconds(1))
    .execute(env.identity.pool())
    .await
    .unwrap();
    // Gone from both lists at once, before the job runs.
    assert!(list(&env, &alice, "incoming").await.is_empty());
    assert!(list(&env, &bob, "outgoing").await.is_empty());
    let response = call(
        &env,
        "POST",
        &format!("/v1/friend-requests/{id}/accept"),
        &alice,
        None,
    )
    .await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;

    // The periodic job marks it expired (another test's janitor may have got there first)...
    janitor(&env).run_once().await.unwrap();
    assert_eq!(request_status(&env, &id).await, "expired");
    // ...and deletes it once it is older than the retention.
    sqlx::query(
        "UPDATE social.friend_requests SET created_at = $2, expires_at = $3 WHERE id = $1::uuid",
    )
    .bind(&id)
    .bind(now - Duration::days(45))
    .bind(now - Duration::days(31))
    .execute(env.identity.pool())
    .await
    .unwrap();
    janitor(&env).run_once().await.unwrap();
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM social.friend_requests WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn reset_invites_are_purged_after_the_retention() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    invite_token(&env, &alice).await;
    call(&env, "POST", "/v1/me/invite/reset", &alice, None).await;
    sqlx::query(
        "UPDATE social.invites SET revoked_at = $2 WHERE owner_id = $1 AND revoked_at IS NOT NULL",
    )
    .bind(alice.id)
    .bind(env.clock.now() - Duration::days(31))
    .execute(env.identity.pool())
    .await
    .unwrap();
    janitor(&env).run_once().await.unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM social.invites WHERE owner_id = $1")
        .bind(alice.id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(rows, 1, "only the live invite is left");
}
