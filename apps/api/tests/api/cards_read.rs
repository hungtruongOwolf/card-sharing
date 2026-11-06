//! T-79: one scoped visibility predicate decides who reads a card. Owner, or a recipient it was
//! delivered to while still friends, unblocked and the card active; everyone else gets a 404
//! byte-identical to a missing id. Originals are owner-only.

use crate::cards_send::{card_body, clean_media, post_card};
use crate::common::*;
use crate::media::without_request_id;
use crate::media_gate::fake_gates;
use crate::media_process::{real_png, real_png_with, uploaded_id};
use crate::social_support::*;
use http::StatusCode;
use image::Rgba;
use media::safety::FAKE_NUDITY_MARKER;
use serde_json::{Value, json};
use uuid::Uuid;

pub struct Delivered {
    pub owner: User,
    pub friend: User,
    pub card: Uuid,
}

/// A card from a new owner to a new friend, with an original, delivered (both gates passed).
pub async fn delivered_card(env: &TestEnv) -> Delivered {
    let owner = adult(env).await;
    let friend = adult(env).await;
    make_friends(env, owner.id, friend.id).await;
    let card = deliver_to(env, &owner, &[&friend]).await;
    Delivered {
        owner,
        friend,
        card,
    }
}

/// Sends a delivered card (with a kept original) from `owner` to `recipients`.
pub async fn deliver_to(env: &TestEnv, owner: &User, recipients: &[&User]) -> Uuid {
    let (render, mask) = clean_media(env, owner).await;
    let original = uploaded_id(env, owner, "original", real_png(120, 90)).await;
    env.state
        .media
        .process_one(original, &fake_gates())
        .await
        .unwrap();
    let ids: Vec<String> = recipients.iter().map(|u| u.id.to_string()).collect();
    let mut body = card_body(
        render,
        Some(mask),
        json!({ "mode": "subset", "recipient_ids": ids }),
    );
    body["original_media_id"] = json!(original);
    let response = post_card(env, owner, &body).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let card = Uuid::parse_str(json_body(response).await["card"]["id"].as_str().unwrap()).unwrap();
    env.state
        .cards
        .evaluate(&env.state.media, card)
        .await
        .unwrap();
    card
}

pub async fn read(env: &TestEnv, user: &User, card: Uuid) -> http::Response<axum::body::Body> {
    call(env, "GET", &format!("/v1/cards/{card}"), user, None).await
}

/// Asserts a 404 that is byte-identical (request id aside) to a missing card.
pub async fn assert_hidden(env: &TestEnv, user: &User, card: Uuid) {
    let denied = read(env, user, card).await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    let denied = without_request_id(body_bytes(denied).await);
    let missing = read(env, user, Uuid::now_v7()).await;
    assert_eq!(without_request_id(body_bytes(missing).await), denied);
}

#[tokio::test]
async fn owner_and_recipient_see_their_own_view() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let d = delivered_card(&env).await;

    let mine = json_body(read(&env, &d.owner, d.card).await).await;
    let card = &mine["card"];
    assert_eq!(card["is_mine"], true);
    assert_eq!(card["status"], "active");
    assert_eq!(card["caption"], "zoomies");
    assert!(
        card["media"]["render_url"]
            .as_str()
            .unwrap()
            .starts_with(TEST_MEDIA_BASE_URL)
    );
    assert!(card["media"]["mask_url"].is_string());
    assert!(
        card["media"]["original_url"].is_string(),
        "owner gets the original"
    );

    let theirs = read(&env, &d.friend, d.card).await;
    assert_eq!(theirs.status(), StatusCode::OK);
    let theirs = json_body(theirs).await;
    let card = &theirs["card"];
    assert_eq!(card["is_mine"], false);
    assert_eq!(card["sender"]["id"], d.owner.id.to_string());
    assert_eq!(card["sender"]["display_name"], d.owner.name.as_str());
    assert!(card.get("status").is_none());
    assert!(card.get("hold").is_none());
    assert!(
        card["media"].get("original_url").is_none(),
        "recipients never get the original"
    );
    // URLs are signed for the viewer, not shared between viewers.
    assert_ne!(
        mine["card"]["media"]["render_url"],
        card["media"]["render_url"]
    );
    let url = card["media"]["render_url"].as_str().unwrap();
    assert!(url.contains(&format!("v={}", d.friend.id.simple())));
    // The URL works at the edge.
    let path = url.strip_prefix("http://localhost").unwrap();
    let image = send(&env.router, get_request(path, None)).await;
    assert_eq!(image.status(), StatusCode::OK);
}

#[tokio::test]
async fn third_parties_unfriended_and_blocked_users_get_the_missing_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let d = delivered_card(&env).await;
    let stranger = adult(&env).await;
    assert_hidden(&env, &stranger, d.card).await;

    // A unfriends B: B loses the card on the next read.
    let response = call(
        &env,
        "DELETE",
        &format!("/v1/friends/{}", d.friend.id),
        &d.owner,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_hidden(&env, &d.friend, d.card).await;
    // The owner still sees it.
    assert_eq!(read(&env, &d.owner, d.card).await.status(), StatusCode::OK);

    // Blocks either way hide it too (friends again, then a block).
    let d2 = delivered_card(&env).await;
    block(&env, d2.friend.id, d2.owner.id).await;
    assert_hidden(&env, &d2.friend, d2.card).await;
    let d3 = delivered_card(&env).await;
    block(&env, d3.owner.id, d3.friend.id).await;
    assert_hidden(&env, &d3.friend, d3.card).await;
}

#[tokio::test]
async fn a_suspended_sender_or_a_deleted_card_is_hidden() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let d = delivered_card(&env).await;
    sqlx::query("UPDATE identity.accounts SET status = 'suspended' WHERE id = $1")
        .bind(d.owner.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    assert_hidden(&env, &d.friend, d.card).await;

    let d2 = delivered_card(&env).await;
    sqlx::query("UPDATE cards.cards SET status = 'deleted' WHERE id = $1")
        .bind(d2.card)
        .execute(env.identity.pool())
        .await
        .unwrap();
    assert_hidden(&env, &d2.friend, d2.card).await;
    assert_hidden(&env, &d2.owner, d2.card).await;
}

#[tokio::test]
async fn pending_and_held_cards_are_owner_only_without_media() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let friend = adult(&env).await;
    make_friends(&env, owner.id, friend.id).await;

    // Pending: media not scanned yet.
    let render = uploaded_id(&env, &owner, "card_render", real_png(40, 40)).await;
    let body = card_body(
        render,
        None,
        json!({ "mode": "subset", "recipient_ids": [friend.id] }),
    );
    let pending: Value = json_body(post_card(&env, &owner, &body).await).await;
    let pending = Uuid::parse_str(pending["card"]["id"].as_str().unwrap()).unwrap();
    let view = json_body(read(&env, &owner, pending).await).await;
    assert_eq!(view["card"]["status"], "pending_scan");
    assert!(view["card"]["media"].is_null());
    assert_hidden(&env, &friend, pending).await;

    // Held by the classifier: the owner sees the hold block, recipients nothing.
    let flagged = uploaded_id(
        &env,
        &owner,
        "card_render",
        real_png_with(40, 40, Rgba(FAKE_NUDITY_MARKER)),
    )
    .await;
    env.state
        .media
        .process_one(flagged, &fake_gates())
        .await
        .unwrap();
    let body = card_body(
        flagged,
        None,
        json!({ "mode": "subset", "recipient_ids": [friend.id] }),
    );
    let held: Value = json_body(post_card(&env, &owner, &body).await).await;
    let held = Uuid::parse_str(held["card"]["id"].as_str().unwrap()).unwrap();
    env.state
        .cards
        .evaluate(&env.state.media, held)
        .await
        .unwrap();
    let view = json_body(read(&env, &owner, held).await).await;
    assert_eq!(view["card"]["status"], "held_review");
    assert_eq!(view["card"]["hold"]["reason"], "nudity_classifier");
    assert_eq!(view["card"]["hold"]["appeal"], "none");
    assert!(
        view["card"]["media"].is_null(),
        "no URL for a held image, even to the owner"
    );
    assert_hidden(&env, &friend, held).await;
}

#[tokio::test]
async fn malformed_ids_are_the_same_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let response = call(&env, "GET", "/v1/cards/not-a-uuid", &user, None).await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}
