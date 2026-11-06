//! Pin-add authorization (T-100, AC-19.7, SR-6): a pinner must be able to view the card now.
//! Collaborators pin only their own active cards; the owner may also pin cards they received and
//! can still see (still friends, no block, card active).

use crate::board_ops::{add_card, add_collaborator, apply, setup};
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::social_support::*;
use serde_json::json;
use uuid::Uuid;

async fn set_status(env: &TestEnv, card: Uuid, status: &str) {
    sqlx::query("UPDATE cards.cards SET status = $2 WHERE id = $1")
        .bind(card)
        .bind(status)
        .execute(env.identity.pool())
        .await
        .unwrap();
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
async fn the_owner_pins_received_cards_only_while_they_can_view_them() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    // A card the owner received from a friend.
    let received = deliver_to(&env, &s.friend, &[&s.owner]).await;
    let pin = |card| {
        apply(
            &env,
            &s.owner,
            s.board,
            json!([add_card(Uuid::now_v7(), card)]),
        )
    };
    assert_eq!(pin(received).await["results"][0]["status"], "applied");

    unfriend(&env, s.owner.id, s.friend.id).await;
    assert_eq!(pin(received).await["results"][0]["code"], "NOT_FOUND");

    make_friends(&env, s.owner.id, s.friend.id).await;
    block(&env, s.friend.id, s.owner.id).await;
    assert_eq!(pin(received).await["results"][0]["code"], "NOT_FOUND");

    // A card the owner was never sent, from a friend of theirs.
    let third = adult(&env).await;
    make_friends(&env, s.owner.id, third.id).await;
    let other = adult(&env).await;
    make_friends(&env, third.id, other.id).await;
    let not_sent = deliver_to(&env, &third, &[&other]).await;
    assert_eq!(pin(not_sent).await["results"][0]["code"], "NOT_FOUND");
    // A received card that was deleted.
    let deleted = deliver_to(&env, &third, &[&s.owner]).await;
    set_status(&env, deleted, "deleted").await;
    assert_eq!(pin(deleted).await["results"][0]["code"], "NOT_FOUND");
}

async fn pin_as(env: &TestEnv, user: &User, board: Uuid, card: Uuid) -> serde_json::Value {
    apply(env, user, board, json!([add_card(Uuid::now_v7(), card)])).await
}

#[tokio::test]
async fn collaborators_pin_only_their_own_cards_and_held_cards_are_refused() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let collaborator = s.friend.clone();
    add_collaborator(&env, s.board, &collaborator).await;
    // The collaborator received the owner's card and may view it, but may not pin it.
    assert_eq!(
        pin_as(&env, &collaborator, s.board, s.card).await["results"][0]["code"],
        "NOT_FOUND"
    );
    let own = deliver_to(&env, &collaborator, &[&s.owner]).await;
    assert_eq!(
        pin_as(&env, &collaborator, s.board, own).await["results"][0]["status"],
        "applied"
    );

    // Held for review: its owner gets the dedicated code, anyone else the missing one.
    set_status(&env, own, "held_review").await;
    assert_eq!(
        pin_as(&env, &collaborator, s.board, own).await["results"][0]["code"],
        "CARD_HELD_FOR_REVIEW"
    );
    assert_eq!(
        pin_as(&env, &s.owner, s.board, own).await["results"][0]["code"],
        "NOT_FOUND"
    );
    set_status(&env, s.card, "pending_scan").await;
    assert_eq!(
        pin_as(&env, &s.owner, s.board, s.card).await["results"][0]["code"],
        "NOT_FOUND"
    );
}
