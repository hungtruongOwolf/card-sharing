//! The minor rule on shared boards (T-102, AC-19.9, SR-21): an adult member who is not friends
//! with a minor member sees that minor's pins as private tiles and their display name only.

use crate::board_ops::{add_card, add_collaborator, apply};
use crate::board_read::{assert_no_card_data, pin_of, read_board};
use crate::boards::new_board;
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

async fn members(env: &TestEnv, user: &User, board: Uuid) -> Vec<Value> {
    let response = call(
        env,
        "GET",
        &format!("/v1/boards/{board}/members"),
        user,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["items"]
        .as_array()
        .unwrap()
        .clone()
}

fn member<'a>(list: &'a [Value], user: &User) -> &'a Value {
    list.iter()
        .find(|m| m["id"] == user.id.to_string())
        .unwrap()
}

#[tokio::test]
async fn an_adult_who_is_not_friends_with_a_minor_pinner_gets_a_private_tile() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    // `adult` received the card from its sender, so the card-read rule alone would show it.
    let minor_owner = minor(&env).await;
    let sender = adult(&env).await;
    let viewer = adult(&env).await;
    make_friends(&env, minor_owner.id, sender.id).await;
    make_friends(&env, sender.id, viewer.id).await;
    let card = deliver_to(&env, &sender, &[&minor_owner, &viewer]).await;
    let board = new_board(&env, &minor_owner).await;
    add_collaborator(&env, board, &viewer).await;
    let pin = Uuid::now_v7();
    apply(&env, &minor_owner, board, json!([add_card(pin, card)])).await;

    assert_eq!(
        pin_of(&read_board(&env, &minor_owner, board, "").await, pin)["kind"],
        "card"
    );
    let body = read_board(&env, &viewer, board, "").await;
    assert_eq!(pin_of(&body, pin)["kind"], "private_tile");
    assert_no_card_data(&body, card);

    // Becoming friends with the minor lifts the rule on the next read.
    make_friends(&env, viewer.id, minor_owner.id).await;
    assert_eq!(
        pin_of(&read_board(&env, &viewer, board, "").await, pin)["kind"],
        "card"
    );
}

#[tokio::test]
async fn the_member_list_shows_a_minor_by_display_name_only_to_adult_non_friends() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let teen = minor(&env).await;
    let stranger = adult(&env).await;
    make_friends(&env, owner.id, teen.id).await;
    make_friends(&env, owner.id, stranger.id).await;
    let board = new_board(&env, &owner).await;
    add_collaborator(&env, board, &teen).await;
    add_collaborator(&env, board, &stranger).await;

    let seen_by_stranger = members(&env, &stranger, board).await;
    assert_eq!(seen_by_stranger.len(), 3);
    assert_eq!(seen_by_stranger[0]["role"], "owner");
    let teen_view = member(&seen_by_stranger, &teen);
    assert_eq!(teen_view["display_name"], teen.name);
    assert!(teen_view.get("handle").is_none());
    assert!(!json!(seen_by_stranger).to_string().contains(&teen.handle));
    assert_eq!(member(&seen_by_stranger, &owner)["handle"], owner.handle);

    // The owner is the teen's friend; the teen is a minor viewer: both see full profiles.
    assert_eq!(
        member(&members(&env, &owner, board).await, &teen)["handle"],
        teen.handle
    );
    let seen_by_teen = members(&env, &teen, board).await;
    assert_eq!(member(&seen_by_teen, &stranger)["handle"], stranger.handle);
    assert_eq!(member(&seen_by_teen, &teen)["is_me"], true);
    assert_eq!(member(&seen_by_teen, &teen)["handle"], teen.handle);

    // Non-members get the missing 404.
    let outsider = adult(&env).await;
    let response = call(
        &env,
        "GET",
        &format!("/v1/boards/{board}/members"),
        &outsider,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
