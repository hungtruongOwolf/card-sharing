//! T-148: appeals of classifier holds and expiry of unappealed ones (US-28 AC-28.3), plus the
//! moderator decisions `release_hold` (the card is delivered) and `uphold_hold` (it is blocked).

use crate::cards_deliver::{Setup, delivered_to, pending_card, status};
use crate::common::*;
use crate::media_gate::fake_gates;
use crate::moderation_actions::act;
use crate::social_support::*;
use crate::staff_auth::{signed_in_staff, staff_member};
use cards::Evaluation;
use http::StatusCode;
use identity::StaffRole;
use media::safety::FAKE_NUDITY_MARKER;
use moderation::{ModerationQueue, TargetType};
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

/// A card whose render the classifier held.
async fn held_card(env: &TestEnv) -> Setup {
    let s = pending_card(env, Some(FAKE_NUDITY_MARKER)).await;
    for id in [s.render, s.mask] {
        env.state
            .media
            .process_one(id, &fake_gates())
            .await
            .unwrap();
    }
    assert_eq!(
        env.state
            .cards
            .evaluate(&env.state.media, s.card)
            .await
            .unwrap(),
        Evaluation::Held
    );
    s
}

async fn appeal(
    env: &TestEnv,
    user: &User,
    card: Uuid,
    body: &Value,
) -> http::Response<axum::body::Body> {
    call(
        env,
        "POST",
        &format!("/v1/cards/{card}/appeal"),
        user,
        Some(body),
    )
    .await
}

async fn hold_status(env: &TestEnv, card: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM moderation.content_holds WHERE card_id = $1")
        .bind(card)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn the_owner_appeals_once_and_the_appeal_joins_the_queue() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = held_card(&env).await;
    let response = appeal(&env, &s.sender, s.card, &json!({"note": "it is my cat"})).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = json_body(response).await;
    assert_eq!(body["appeal"]["status"], "pending");
    assert_eq!(hold_status(&env, s.card).await, "appealed");
    let item = ModerationQueue::new(env.identity.pool().clone())
        .live_for(TargetType::Card, s.card)
        .await
        .unwrap()
        .expect("the appeal is a queue item");
    assert_eq!(item.target_owner_id, s.sender.id);
    let (source, note): (String, Option<String>) =
        sqlx::query_as("SELECT source, note FROM moderation.reports WHERE queue_item_id = $1")
            .bind(item.id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(
        (source.as_str(), note.as_deref()),
        ("appeal", Some("it is my cat"))
    );

    // A second appeal: 409 APPEAL_ALREADY_FILED.
    let again = appeal(&env, &s.sender, s.card, &json!({})).await;
    assert_problem(again, StatusCode::CONFLICT, "APPEAL_ALREADY_FILED").await;
}

#[tokio::test]
async fn only_the_owner_of_a_held_card_can_appeal() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = held_card(&env).await;
    // The recipient and a stranger: the missing 404.
    let stranger = adult(&env).await;
    for user in [&s.friend, &stranger] {
        let response = appeal(&env, user, s.card, &json!({})).await;
        assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    }
    // A card that is not held, and a malformed id.
    let clean = pending_card(&env, None).await;
    let response = appeal(&env, &clean.sender, clean.card, &json!({})).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = call(
        &env,
        "POST",
        "/v1/cards/not-a-card/appeal",
        &s.sender,
        Some(&json!({})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    // A note longer than 500 characters.
    let long = appeal(&env, &s.sender, s.card, &json!({"note": "x".repeat(501)})).await;
    assert_eq!(long.status(), StatusCode::BAD_REQUEST);
    // None of the refused calls filed an appeal. Not `== "held"`: `expire_holds` is global and the
    // expiry test, running concurrently on the same database with its clock 31 days ahead, may
    // expire this hold too (#361).
    assert_ne!(hold_status(&env, s.card).await, "appealed");
}

#[tokio::test]
async fn release_hold_delivers_the_card() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = held_card(&env).await;
    appeal(&env, &s.sender, s.card, &json!({})).await;
    let item = ModerationQueue::new(env.identity.pool().clone())
        .live_for(TargetType::Card, s.card)
        .await
        .unwrap()
        .unwrap();
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &env.router, &moderator).await;
    let response = act(
        &env,
        &session,
        item.id,
        &json!({"action": "release_hold", "reason": "false positive"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(hold_status(&env, s.card).await, "released");
    assert_eq!(status(&env, s.card).await, "active");
    assert_eq!(delivered_to(&env, s.card).await, 1);
    let state: String = sqlx::query_scalar("SELECT state FROM media.media_objects WHERE id = $1")
        .bind(s.render)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(state, "clean");
}

#[tokio::test]
async fn uphold_hold_blocks_the_card() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = held_card(&env).await;
    appeal(&env, &s.sender, s.card, &json!({})).await;
    let item = ModerationQueue::new(env.identity.pool().clone())
        .live_for(TargetType::Card, s.card)
        .await
        .unwrap()
        .unwrap();
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &env.router, &moderator).await;
    let response = act(
        &env,
        &session,
        item.id,
        &json!({"action": "uphold_hold", "reason": "nudity"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(hold_status(&env, s.card).await, "upheld");
    assert_eq!(status(&env, s.card).await, "blocked");
    assert_eq!(delivered_to(&env, s.card).await, 0);
}

#[tokio::test]
async fn unappealed_holds_expire_after_30_days_and_their_card_is_deleted() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let unappealed = held_card(&env).await;
    let appealed = held_card(&env).await;
    appeal(&env, &appealed.sender, appealed.card, &json!({})).await;
    env.clock.advance(Duration::days(29));
    env.state.cards.expire_holds().await.unwrap();
    assert_eq!(hold_status(&env, unappealed.card).await, "held");
    env.clock.advance(Duration::days(2));
    assert!(env.state.cards.expire_holds().await.unwrap() >= 1);
    assert_eq!(hold_status(&env, unappealed.card).await, "expired");
    assert_eq!(status(&env, unappealed.card).await, "deleted");
    // An appealed hold waits for a moderator.
    assert_eq!(hold_status(&env, appealed.card).await, "appealed");
    // Too late to appeal an expired hold (a fresh session: the old access token expired meanwhile).
    let owner = signed_in_again(&env, &unappealed.sender).await;
    let late = appeal(&env, &owner, unappealed.card, &json!({})).await;
    assert_eq!(late.status(), StatusCode::NOT_FOUND);
}
