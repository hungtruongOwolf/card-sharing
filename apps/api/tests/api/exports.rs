//! Personal data export (T-154, US-30 AC-30.1/30.2, SR-36): re-auth, one per 24 h, a 48 h delay
//! that can be cancelled, a ZIP of every module's data and images, a single-use download link,
//! and nothing reachable by another account.

use crate::cards_read::deliver_to;
use crate::common::*;
use crate::media::without_request_id;
use crate::reauth::reauth_token;
use crate::social_support::*;
use axum::body::Body;
use http::{Request, Response, StatusCode};
use media::{Bucket, ObjectKey, ObjectStore};
use serde_json::Value;
use time::Duration;
use uuid::Uuid;

async fn with_reauth(
    env: &TestEnv,
    method: &str,
    path: &str,
    user: &User,
    reauth: Option<&str>,
) -> Response<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {}", user.token));
    if let Some(reauth) = reauth {
        builder = builder.header("x-reauth", reauth);
    }
    send(&env.router, builder.body(Body::empty()).unwrap()).await
}

/// Requests an export (fresh re-auth) and returns its id.
async fn requested(env: &TestEnv, user: &User) -> Uuid {
    let reauth = reauth_token(env, user).await;
    let response = with_reauth(env, "POST", "/v1/me/exports", user, Some(&reauth)).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = json_body(response).await;
    assert_eq!(body["export"]["status"], "queued");
    Uuid::parse_str(body["export"]["id"].as_str().unwrap()).unwrap()
}

async fn status_of(env: &TestEnv, user: &User, id: Uuid) -> Value {
    let reauth = reauth_token(env, user).await;
    let response = with_reauth(
        env,
        "GET",
        &format!("/v1/me/exports/{id}"),
        user,
        Some(&reauth),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["export"].clone()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[tokio::test]
async fn an_export_is_built_after_the_delay_and_downloaded_once() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let friend = adult(&env).await;
    make_friends(&env, user.id, friend.id).await;
    let card = deliver_to(&env, &user, &[&friend]).await;

    let without = with_reauth(&env, "POST", "/v1/me/exports", &user, None).await;
    assert_problem(without, StatusCode::UNAUTHORIZED, "REAUTH_REQUIRED").await;
    let id = requested(&env, &user).await;
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.outbox_events
         WHERE topic = 'privacy.export_requested' AND payload->>'export_id' = $1",
    )
    .bind(id.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(events, 1, "every device can be told");

    // AC-30.2: one per 24 hours.
    let reauth = reauth_token(&env, &user).await;
    let again = with_reauth(&env, "POST", "/v1/me/exports", &user, Some(&reauth)).await;
    assert!(again.headers().contains_key("retry-after"));
    assert_problem(again, StatusCode::TOO_MANY_REQUESTS, "EXPORT_COOLDOWN").await;

    // Nothing is built during the 48 h delay. (Tests build their own export by id: the shared
    // database holds other tests' exports, each with its own manual clock.)
    let exports = env.state.exports();
    assert!(!exports.build(id).await.unwrap());
    assert_eq!(status_of(&env, &user, id).await["status"], "queued");

    env.clock
        .advance(Duration::hours(48) + Duration::seconds(1));
    let user = signed_in_again(&env, &user).await;
    assert!(exports.build(id).await.unwrap());
    let export = status_of(&env, &user, id).await;
    assert_eq!(export["status"], "ready", "{export}");
    let path = export["download_path"].as_str().unwrap().to_owned();

    let download = send(&env.router, get_request(&path, None)).await;
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(download.headers()["content-type"], "application/zip");
    let zip = body_bytes(download).await;
    assert_eq!(&zip[..4], b"PK\x03\x04");
    for name in [
        "identity.json",
        "social.json",
        "cards.json",
        "boards.json",
        "media/card_render/",
        "media/mask/",
        "media/original/",
    ] {
        assert!(contains(&zip, name.as_bytes()), "{name} missing");
    }
    assert!(contains(&zip, b"\x89PNG"), "images are PNG");
    assert!(contains(&zip, friend.handle.as_bytes()), "friends by name");
    assert!(contains(&zip, card.to_string().as_bytes()));

    // Single use.
    let reused = send(&env.router, get_request(&path, None)).await;
    assert_problem(reused, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    let export = status_of(&env, &user, id).await;
    assert_eq!(export["status"], "downloaded");
    assert!(export.get("download_path").is_none());
}

#[tokio::test]
async fn a_new_link_replaces_the_previous_one_and_files_expire() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let id = requested(&env, &user).await;
    env.clock
        .advance(Duration::hours(48) + Duration::seconds(1));
    let user = signed_in_again(&env, &user).await;
    let exports = env.state.exports();
    assert!(exports.build(id).await.unwrap());
    let first = status_of(&env, &user, id).await["download_path"]
        .as_str()
        .unwrap()
        .to_owned();
    let second = status_of(&env, &user, id).await["download_path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(first, second);
    let stale = send(&env.router, get_request(&first, None)).await;
    assert_problem(stale, StatusCode::NOT_FOUND, "NOT_FOUND").await;

    let key: String =
        sqlx::query_scalar("SELECT storage_key FROM privacy.data_exports WHERE id = $1")
            .bind(id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    env.clock
        .advance(Duration::hours(24) + Duration::seconds(1));
    assert_eq!(exports.expire_one(id).await.unwrap(), 1);
    let expired = send(&env.router, get_request(&second, None)).await;
    assert_problem(expired, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    let object = env
        .media_store
        .get(Bucket::Clean, &ObjectKey::parse(&key).unwrap())
        .await
        .unwrap();
    assert!(object.is_none(), "the ZIP is deleted at expiry");
}

#[tokio::test]
async fn a_queued_export_can_be_cancelled_and_then_requested_again() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let id = requested(&env, &user).await;
    let cancelled = call(&env, "DELETE", &format!("/v1/me/exports/{id}"), &user, None).await;
    assert_eq!(cancelled.status(), StatusCode::NO_CONTENT);
    let again = call(&env, "DELETE", &format!("/v1/me/exports/{id}"), &user, None).await;
    assert_problem(again, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    // A cancelled export does not count against the cooldown.
    let next = requested(&env, &user).await;
    assert_ne!(next, id);
    env.clock.advance(Duration::hours(49));
    let user = signed_in_again(&env, &user).await;
    assert!(!env.state.exports().build(id).await.unwrap());
    assert_eq!(status_of(&env, &user, id).await["status"], "cancelled");
}

#[tokio::test]
async fn another_accounts_export_is_not_found() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let other = adult(&env).await;
    let id = requested(&env, &owner).await;
    let reauth = reauth_token(&env, &other).await;
    let read = with_reauth(
        &env,
        "GET",
        &format!("/v1/me/exports/{id}"),
        &other,
        Some(&reauth),
    )
    .await;
    assert_eq!(read.status(), StatusCode::NOT_FOUND);
    let read = without_request_id(body_bytes(read).await);
    let reauth = reauth_token(&env, &other).await;
    let missing = with_reauth(
        &env,
        "GET",
        &format!("/v1/me/exports/{}", Uuid::now_v7()),
        &other,
        Some(&reauth),
    )
    .await;
    assert_eq!(without_request_id(body_bytes(missing).await), read);
    let cancel = call(
        &env,
        "DELETE",
        &format!("/v1/me/exports/{id}"),
        &other,
        None,
    )
    .await;
    assert_problem(cancel, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    let status: String =
        sqlx::query_scalar("SELECT status FROM privacy.data_exports WHERE id = $1")
            .bind(id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(status, "queued");
}
