//! Board reads resolved per viewer with delta sync (T-101, AC-19.4, AC-19.7, SR-6): a pin never
//! reveals a card, its caption or a media URL to a member who may not view that card.

use crate::board_ops::{add_card, add_collaborator, apply, remove, setup};
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn read_board(env: &TestEnv, user: &User, board: Uuid, query: &str) -> Value {
    let response = call(
        env,
        "GET",
        &format!("/v1/boards/{board}{query}"),
        user,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

pub fn pin_of(body: &Value, pin: Uuid) -> &Value {
    body["pins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == pin.to_string())
        .unwrap_or_else(|| panic!("pin {pin} missing from {body}"))
}

/// Asserts the body carries nothing about `card`: no id, caption or media URL anywhere.
pub fn assert_no_card_data(body: &Value, card: Uuid) {
    let text = body.to_string();
    assert!(!text.contains(&card.to_string()), "card id leaked: {text}");
    assert!(!text.contains("zoomies"), "caption leaked: {text}");
    assert!(!text.contains("render_url"), "media leaked: {text}");
    assert!(
        !text.contains("/v1/media-files/"),
        "media URL leaked: {text}"
    );
}

async fn unfriend(env: &TestEnv, a: Uuid, b: Uuid) {
    sqlx::query("DELETE FROM social.friendships WHERE user_a = LEAST($1::uuid, $2::uuid) AND user_b = GREATEST($1::uuid, $2::uuid)")
        .bind(a)
        .bind(b)
        .execute(env.identity.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn card_pins_show_only_to_members_who_may_view_the_card() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    // The owner's card was sent to `friend` only.
    let s = setup(&env).await;
    let outsider = adult(&env).await;
    make_friends(&env, s.owner.id, outsider.id).await;
    add_collaborator(&env, s.board, &s.friend).await;
    add_collaborator(&env, s.board, &outsider).await;
    let pin = Uuid::now_v7();
    apply(&env, &s.owner, s.board, json!([add_card(pin, s.card)])).await;

    for viewer in [&s.owner, &s.friend] {
        let body = read_board(&env, viewer, s.board, "").await;
        assert_eq!(body["full"], true);
        let view = pin_of(&body, pin);
        assert_eq!(view["kind"], "card");
        assert_eq!(view["card"]["id"], s.card.to_string());
        assert!(view["card"]["media"]["render_url"].is_string());
    }
    // AC-19.7: a collaborator who is not a recipient gets a private tile without any card data.
    let body = read_board(&env, &outsider, s.board, "").await;
    let view = pin_of(&body, pin);
    assert_eq!(view["kind"], "private_tile");
    assert_eq!(view["mine"], false);
    assert!(view["x"].is_number());
    assert_no_card_data(&body, s.card);

    // An unfriended recipient sees a private tile on the next read, full or delta.
    unfriend(&env, s.owner.id, s.friend.id).await;
    let version = body["board"]["version"].as_i64().unwrap();
    for query in [String::new(), format!("?since_version={version}")] {
        let body = read_board(&env, &s.friend, s.board, &query).await;
        assert_eq!(pin_of(&body, pin)["kind"], "private_tile");
        assert_no_card_data(&body, s.card);
    }
    // A block either way hides it too.
    make_friends(&env, s.owner.id, s.friend.id).await;
    assert_eq!(
        pin_of(&read_board(&env, &s.friend, s.board, "").await, pin)["kind"],
        "card"
    );
    block(&env, s.friend.id, s.owner.id).await;
    let body = read_board(&env, &s.friend, s.board, "").await;
    assert_eq!(pin_of(&body, pin)["kind"], "private_tile");
    assert_no_card_data(&body, s.card);
}

#[tokio::test]
async fn deltas_return_changes_tombstones_and_every_live_card_pin() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let kept = Uuid::now_v7();
    let dropped = Uuid::now_v7();
    apply(
        &env,
        &s.owner,
        s.board,
        json!([add_card(kept, s.card), add_card(dropped, s.card)]),
    )
    .await;
    let body = read_board(&env, &s.owner, s.board, "").await;
    let version = body["board"]["version"].as_i64().unwrap();
    assert_eq!(version, 3);
    assert_eq!(body["pins"].as_array().unwrap().len(), 2);

    apply(&env, &s.owner, s.board, json!([remove(dropped, 1)])).await;
    let delta = read_board(
        &env,
        &s.owner,
        s.board,
        &format!("?since_version={version}"),
    )
    .await;
    assert_eq!(delta["full"], false);
    assert_eq!(delta["board"]["version"], 4);
    let tombstone = pin_of(&delta, dropped);
    assert_eq!(tombstone["kind"], "removed");
    assert!(tombstone.get("x").is_none() && tombstone.get("card").is_none());
    assert_eq!(pin_of(&delta, kept)["kind"], "card");
    // A full read never lists tombstones.
    let full = read_board(&env, &s.owner, s.board, "").await;
    assert_eq!(full["pins"].as_array().unwrap().len(), 1);
    // A cursor ahead of the board (e.g. a stale client) gets a full snapshot.
    let ahead = read_board(&env, &s.owner, s.board, "?since_version=999").await;
    assert_eq!(ahead["full"], true);

    // A deleted card's pins are removed for everyone.
    let response = call(
        &env,
        "DELETE",
        &format!("/v1/cards/{}", s.card),
        &s.owner,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let body = read_board(&env, &s.owner, s.board, "").await;
    assert_eq!(pin_of(&body, kept)["kind"], "removed");
    assert_no_card_data(&body, s.card);

    for query in ["?since_version=-1", "?since_version=abc", "?other=1"] {
        let response = call(
            &env,
            "GET",
            &format!("/v1/boards/{}{query}", s.board),
            &s.owner,
            None,
        )
        .await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
}

#[tokio::test]
async fn pinned_received_cards_follow_the_card_read_rules() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    // The owner pins a card a friend sent them; a collaborator who never received it sees a tile.
    let s = setup(&env).await;
    let received = deliver_to(&env, &s.friend, &[&s.owner]).await;
    let collaborator = adult(&env).await;
    make_friends(&env, s.owner.id, collaborator.id).await;
    make_friends(&env, s.friend.id, collaborator.id).await;
    add_collaborator(&env, s.board, &collaborator).await;
    let pin = Uuid::now_v7();
    apply(&env, &s.owner, s.board, json!([add_card(pin, received)])).await;
    assert_eq!(
        pin_of(&read_board(&env, &s.owner, s.board, "").await, pin)["kind"],
        "card"
    );
    let body = read_board(&env, &collaborator, s.board, "").await;
    assert_eq!(pin_of(&body, pin)["kind"], "private_tile");
    assert_no_card_data(&body, received);
    // The card's sender sees their own card on the board only as a member; they are not one.
    let response = call(
        &env,
        "GET",
        &format!("/v1/boards/{}", s.board),
        &s.friend,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
