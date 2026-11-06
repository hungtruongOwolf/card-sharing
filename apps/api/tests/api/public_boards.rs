//! Public board pages for the web server (T-131, US-24, SR-16): svc only, uniform 404s for every
//! unusable token, per-IP throttling, per-link anomaly disabling, friends' cards never served.

use std::sync::Arc;

use crate::board_ops::{add_card, apply};
use crate::boards::new_board;
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::public_links::{created_link, token_of};
use crate::social_support::*;
use axum::body::Body;
use http::{Request, Response, StatusCode};
use serde_json::{Value, json};
use sharing::SnapshotBuilder;
use time::Duration;
use uuid::Uuid;

/// A visitor IP no other test (or earlier run) uses, so per-IP buckets start empty.
fn fresh_ip() -> String {
    let bytes = Uuid::now_v7().into_bytes();
    format!("10.{}.{}.{}", bytes[13], bytes[14], bytes[15])
}

fn public_request(path: &str, ip: &str) -> Request<Body> {
    Request::get(path)
        .header("x-request-id", "0192f0a4-0000-7000-8000-000000000131")
        .header("x-service-credential", TEST_SERVICE_CREDENTIAL)
        .header("x-client-ip", ip)
        .body(Body::empty())
        .unwrap()
}

async fn page(env: &TestEnv, token: &str) -> Response<Body> {
    send(
        &env.router,
        public_request(&format!("/v1/public/boards/{token}"), &fresh_ip()),
    )
    .await
}

async fn og(env: &TestEnv, token: &str) -> Response<Body> {
    send(
        &env.router,
        public_request(&format!("/v1/public/boards/{token}/og"), &fresh_ip()),
    )
    .await
}

/// `token` answers exactly like a token that never existed, on both routes.
async fn assert_neutral(env: &TestEnv, token: &str, case: &str) {
    let unknown = body_bytes(page(env, "BBBBBBBBBBBBBBBBBBBBBB").await).await;
    let response = page(env, token).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND, "{case}");
    assert_eq!(body_bytes(response).await, unknown, "{case}: page");
    let unknown_og = body_bytes(og(env, "BBBBBBBBBBBBBBBBBBBBBB").await).await;
    let response = og(env, token).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND, "{case}");
    assert_eq!(body_bytes(response).await, unknown_og, "{case}: og");
}

struct Setup {
    owner: User,
    friend: User,
    board: Uuid,
    own_card: Uuid,
    friend_card: Uuid,
    token: String,
    link_id: String,
}

async fn setup(env: &TestEnv) -> Setup {
    let owner = adult(env).await;
    let friend = adult(env).await;
    make_friends(env, owner.id, friend.id).await;
    let own_card = deliver_to(env, &owner, &[&friend]).await;
    let friend_card = deliver_to(env, &friend, &[&owner]).await;
    let board = new_board(env, &owner).await;
    apply(
        env,
        &owner,
        board,
        json!([
            add_card(Uuid::now_v7(), own_card),
            add_card(Uuid::now_v7(), friend_card)
        ]),
    )
    .await;
    let link = created_link(env, &owner, board, "7d").await;
    Setup {
        owner,
        friend,
        board,
        own_card,
        friend_card,
        token: token_of(&link),
        link_id: link["id"].as_str().unwrap().to_owned(),
    }
}

async fn rebuild(env: &TestEnv, board: Uuid) {
    SnapshotBuilder::new(
        env.identity.pool().clone(),
        env.clock.clone(),
        Arc::clone(&env.state.boards),
        Arc::clone(&env.state.media),
    )
    .rebuild(board)
    .await
    .unwrap();
}

/// The path part of a signed media URL (the test router serves `/v1/media-files`).
fn media_path(url: &str) -> String {
    url.strip_prefix("http://localhost").unwrap().to_owned()
}

#[tokio::test]
async fn guests_see_only_the_owners_cards_with_signed_watermarked_images() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    // No snapshot job has run yet: the first view builds it.
    let response = page(&env, &s.token).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["noindex"], true);
    assert_eq!(body["cache_ttl_s"], 60);
    assert_eq!(body["board"]["title"], "Summer");
    assert_eq!(body["board"]["owner_display_name"], s.owner.name.as_str());
    let pins = body["pins"].as_array().unwrap();
    let cards: Vec<&Value> = pins.iter().filter(|p| p["kind"] == "card").collect();
    let tiles: Vec<&Value> = pins
        .iter()
        .filter(|p| p["kind"] == "private_tile")
        .collect();
    assert_eq!((cards.len(), tiles.len()), (1, 1), "{body}");
    assert!(tiles[0].get("image_url").is_none());
    let text = body.to_string();
    for secret in [
        s.friend_card.to_string(),
        s.friend.id.to_string(),
        s.own_card.to_string(),
        s.owner.id.to_string(),
        s.friend.name.clone(),
    ] {
        assert!(!text.contains(&secret), "{secret} leaked: {text}");
    }
    // The image URL works and serves the watermarked derivative.
    let url = cards[0]["image_url"].as_str().unwrap();
    let image = send(&env.router, get_request(&media_path(url), None)).await;
    assert_eq!(image.status(), StatusCode::OK);
    let decoded = image::load_from_memory(&body_bytes(image).await).unwrap();
    assert!(decoded.width() <= 1080 && decoded.height() <= 1080);

    // The OG route hands out a signed URL of the 1200x630 image.
    let response = og(&env, &s.token).await;
    assert_eq!(response.status(), StatusCode::OK);
    let og_url = json_body(response).await["url"]
        .as_str()
        .unwrap()
        .to_owned();
    let image = send(&env.router, get_request(&media_path(&og_url), None)).await;
    assert_eq!(image.status(), StatusCode::OK);
    let decoded = image::load_from_memory(&body_bytes(image).await).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (1200, 630));

    let views: i64 =
        sqlx::query_scalar("SELECT view_count FROM sharing.public_links WHERE id = $1::uuid")
            .bind(&s.link_id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(views, 1, "only the page counts as a view");
}

#[tokio::test]
async fn a_card_held_after_the_snapshot_is_served_as_a_tile() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    rebuild(&env, s.board).await;
    sqlx::query("UPDATE cards.cards SET status = 'held_review' WHERE id = $1")
        .bind(s.own_card)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let body = json_body(page(&env, &s.token).await).await;
    assert!(
        body["pins"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["kind"] == "private_tile"),
        "{body}"
    );
}

#[tokio::test]
async fn every_unusable_token_is_the_same_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    assert_neutral(&env, "too-short", "malformed").await;
    assert_neutral(&env, "AAAAAAAAAAAAAAAAAAAAAA", "unknown").await;

    // Revoked.
    let s = setup(&env).await;
    assert_eq!(page(&env, &s.token).await.status(), StatusCode::OK);
    let revoked = call(
        &env,
        "DELETE",
        &format!("/v1/public-links/{}", s.link_id),
        &s.owner,
        None,
    )
    .await;
    assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
    assert_neutral(&env, &s.token, "revoked").await;

    // Expired.
    let s = setup(&env).await;
    let link = created_link(&env, &s.owner, s.board, "24h").await;
    let day = token_of(&link);
    assert_eq!(page(&env, &day).await.status(), StatusCode::OK);
    env.clock.advance(Duration::hours(25));
    assert_neutral(&env, &day, "expired").await;
    assert_eq!(
        page(&env, &s.token).await.status(),
        StatusCode::OK,
        "7d link still live"
    );

    // Board deleted.
    let owner = signed_in_again(&env, &s.owner).await;
    let deleted = call(
        &env,
        "DELETE",
        &format!("/v1/boards/{}", s.board),
        &owner,
        None,
    )
    .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    assert_neutral(&env, &s.token, "board deleted").await;

    // Owner suspended.
    let s = setup(&env).await;
    assert_eq!(page(&env, &s.token).await.status(), StatusCode::OK);
    sqlx::query("UPDATE identity.accounts SET status = 'suspended' WHERE id = $1")
        .bind(s.owner.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    assert_neutral(&env, &s.token, "owner suspended").await;
}

#[tokio::test]
async fn random_tokens_all_answer_the_same_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let reference = body_bytes(page(&env, "BBBBBBBBBBBBBBBBBBBBBB").await).await;
    for _ in 0..1000 {
        let digest = <sha2::Sha256 as sha2::Digest>::digest(Uuid::now_v7().as_bytes());
        let token = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            &digest[..16],
        );
        let response = page(&env, &token).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_bytes(response).await, reference);
    }
}

#[tokio::test]
async fn one_visitor_ip_is_throttled() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let ip = fresh_ip();
    for i in 0..60 {
        let response = send(
            &env.router,
            public_request("/v1/public/boards/AAAAAAAAAAAAAAAAAAAAAA", &ip),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "request {i}");
    }
    let response = send(
        &env.router,
        public_request("/v1/public/boards/AAAAAAAAAAAAAAAAAAAAAA", &ip),
    )
    .await;
    assert_problem(response, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await;
}

#[tokio::test]
async fn traffic_over_the_per_link_limit_disables_the_link() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    // Use up the link's whole minute (600 requests) at once: a slow test loop would let the
    // bucket refill while it runs.
    let subject = platform::rate_limit::Subject {
        account: Some(platform::rate_limit::AccountSubject {
            id: Uuid::parse_str(&s.link_id).unwrap(),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
        }),
        device: None,
        ip: None,
    };
    let decision = env
        .state
        .limiter
        .check_cost(api::routes::public_boards::TOKEN_LIMIT, &subject, 600)
        .await
        .unwrap();
    assert_eq!(decision, platform::rate_limit::Decision::Allowed);
    assert_eq!(og(&env, &s.token).await.status(), StatusCode::NOT_FOUND);
    assert_neutral(&env, &s.token, "disabled").await;
    let reason: Option<String> =
        sqlx::query_scalar("SELECT revoked_reason FROM sharing.public_links WHERE id = $1::uuid")
            .bind(&s.link_id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(reason.as_deref(), Some("anomaly"));
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.outbox_events
         WHERE topic = 'sharing.link_disabled' AND payload->>'link_id' = $1",
    )
    .bind(&s.link_id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(events, 1);
}

#[tokio::test]
async fn the_service_credential_is_required_and_bearer_tokens_are_refused() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let request = Request::get(format!("/v1/public/boards/{}", s.token))
        .header("x-client-ip", fresh_ip())
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send(&env.router, request).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let request = Request::get(format!("/v1/public/boards/{}", s.token))
        .header("authorization", format!("Bearer {}", s.owner.token))
        .header("x-client-ip", fresh_ip())
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send(&env.router, request).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // The credential reaches nothing outside /v1/public/.
    let request = Request::get(format!("/v1/boards/{}/public-links", s.board))
        .header("x-service-credential", TEST_SERVICE_CREDENTIAL)
        .header("x-client-ip", fresh_ip())
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send(&env.router, request).await.status(),
        StatusCode::FORBIDDEN
    );
}
