//! T-76 (+ the card side of T-73): a card reaches its recipients only after every one of its
//! media passed both gates; one delivery-log row per recipient and derivative, no image data.

use crate::cards_send::{card_body, post_card};
use crate::common::*;
use crate::media_gate::fake_gates;
use crate::media_process::{real_png, real_png_with, uploaded_id};
use crate::social_support::*;
use cards::{EvaluateOnEvents, Evaluation};
use http::StatusCode;
use image::Rgba;
use media::safety::{FAKE_MATCH_MARKER, FAKE_NUDITY_MARKER};
use platform::clock::Clock;
use platform::outbox::{StoredEvent, Subscriber};
use serde_json::json;
use uuid::Uuid;

pub struct Setup {
    pub sender: User,
    pub friend: User,
    pub card: Uuid,
    pub render: Uuid,
    pub mask: Uuid,
}

/// A card to one friend whose media are uploaded but not processed yet.
pub async fn pending_card(env: &TestEnv, render_marker: Option<[u8; 4]>) -> Setup {
    let sender = adult(env).await;
    let friend = adult(env).await;
    make_friends(env, sender.id, friend.id).await;
    let png = match render_marker {
        Some(marker) => real_png_with(100, 80, Rgba(marker)),
        None => real_png(100, 80),
    };
    let render = uploaded_id(env, &sender, "card_render", png).await;
    let mask = uploaded_id(env, &sender, "mask", real_png(100, 80)).await;
    let response = post_card(
        env,
        &sender,
        &card_body(
            render,
            Some(mask),
            json!({ "mode": "subset", "recipient_ids": [friend.id] }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let card = Uuid::parse_str(json_body(response).await["card"]["id"].as_str().unwrap()).unwrap();
    Setup {
        sender,
        friend,
        card,
        render,
        mask,
    }
}

pub async fn status(env: &TestEnv, card: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM cards.cards WHERE id = $1")
        .bind(card)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

pub async fn delivered_to(env: &TestEnv, card: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM cards.card_recipients WHERE card_id = $1 AND delivered_at IS NOT NULL",
    )
    .bind(card)
    .fetch_one(env.identity.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn delivered_only_after_both_gates_pass_with_a_delivery_log() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = pending_card(&env, None).await;
    let (cards, media) = (&env.state.cards, &env.state.media);
    // Nothing processed: the card waits.
    assert_eq!(
        cards.evaluate(media, s.card).await.unwrap(),
        Evaluation::Waiting
    );
    // Render clean, mask still scanning: still waiting.
    media.process_one(s.render, &fake_gates()).await.unwrap();
    assert_eq!(cards.evaluate_for_media(media, s.render).await.unwrap(), 1);
    assert_eq!(status(&env, s.card).await, "pending_scan");
    assert_eq!(delivered_to(&env, s.card).await, 0);
    // Mask clean: delivered.
    media.process_one(s.mask, &fake_gates()).await.unwrap();
    cards.evaluate_for_media(media, s.mask).await.unwrap();
    assert_eq!(status(&env, s.card).await, "active");
    assert_eq!(delivered_to(&env, s.card).await, 1);

    // Render: feed_1080 + widget_512 + thumb_256; mask: feed_1080 → 4 rows for the one friend.
    let log: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT recipient_id, variant FROM cards.card_deliveries WHERE card_id = $1 ORDER BY variant",
    )
    .bind(s.card)
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(log.len(), 4);
    assert!(log.iter().all(|(r, _)| *r == s.friend.id));
    let event: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM platform.outbox_events WHERE topic = 'cards.delivered' AND payload->>'card_id' = $1",
    )
    .bind(s.card.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(event["recipient_ids"], json!([s.friend.id]));
    // Evaluating again changes nothing.
    assert_eq!(
        cards.evaluate(media, s.card).await.unwrap(),
        Evaluation::Skipped
    );
}

#[tokio::test]
async fn a_flagged_render_holds_the_card_for_review() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = pending_card(&env, Some(FAKE_NUDITY_MARKER)).await;
    let (cards, media) = (&env.state.cards, &env.state.media);
    for id in [s.render, s.mask] {
        media.process_one(id, &fake_gates()).await.unwrap();
    }
    assert_eq!(
        cards.evaluate(media, s.card).await.unwrap(),
        Evaluation::Held
    );
    assert_eq!(status(&env, s.card).await, "held_review");
    assert_eq!(delivered_to(&env, s.card).await, 0);
    let (hold_status, owner, model, expires): (String, Uuid, String, time::OffsetDateTime) =
        sqlx::query_as(
            "SELECT status, owner_id, classifier_model, expires_at FROM moderation.content_holds WHERE card_id = $1",
        )
        .bind(s.card)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!((hold_status.as_str(), owner), ("held", s.sender.id));
    assert_eq!(model, "fake-nudity-1");
    assert!(expires > env.clock.now() + time::Duration::days(29));
    let held_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.outbox_events WHERE topic = 'cards.held' AND payload->>'card_id' = $1",
    )
    .bind(s.card.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(held_events, 1);
}

#[tokio::test]
async fn a_csam_match_blocks_the_card_and_failures_fail_it() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (cards, media) = (&env.state.cards, &env.state.media);
    let blocked = pending_card(&env, Some(FAKE_MATCH_MARKER)).await;
    for id in [blocked.render, blocked.mask] {
        media.process_one(id, &fake_gates()).await.unwrap();
    }
    assert_eq!(
        cards.evaluate(media, blocked.card).await.unwrap(),
        Evaluation::Blocked
    );
    assert_eq!(delivered_to(&env, blocked.card).await, 0);

    let failed = pending_card(&env, None).await;
    let zero = media::ProcessSettings {
        decode_timeout: std::time::Duration::ZERO,
        ..fake_gates()
    };
    media.process_one(failed.render, &zero).await.unwrap();
    assert_eq!(
        cards.evaluate(media, failed.card).await.unwrap(),
        Evaluation::Failed
    );
    assert_eq!(status(&env, failed.card).await, "failed");
}

#[test]
fn submitted_cards_and_settled_media_queue_an_evaluation() {
    let event = |topic: &str, payload: serde_json::Value| StoredEvent {
        id: 1,
        topic: topic.to_owned(),
        payload,
        created_at: time::OffsetDateTime::UNIX_EPOCH,
    };
    let card = Uuid::now_v7();
    let job = EvaluateOnEvents
        .job(&event(
            "cards.submitted",
            json!({ "card_id": card, "owner_id": Uuid::now_v7() }),
        ))
        .unwrap();
    assert_eq!(job.kind, cards::EVALUATE_JOB);
    assert_eq!(job.payload, json!({ "card_id": card }));
    let media = Uuid::now_v7();
    let job = EvaluateOnEvents
        .job(&event(
            "media.settled",
            json!({ "media_id": media, "state": "clean" }),
        ))
        .unwrap();
    assert_eq!(job.payload, json!({ "media_id": media }));
    assert!(EvaluateOnEvents.topics().contains(&media::MEDIA_SETTLED));
}
