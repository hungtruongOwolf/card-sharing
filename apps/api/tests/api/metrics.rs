//! T-97: the minute metrics snapshot measures send-to-delivered latency, scan latency, holds,
//! queue age per lane and push outcomes; an old queued job trips the queue-age alert.

use std::sync::Arc;

use crate::cards_send::{card_body, clean_media, post_card};
use crate::common::*;
use crate::social_support::*;
use api::workers::{Alert, SloReport};
use http::StatusCode;
use platform::clock::Clock;
use platform::jobs::{JobQueue, Lane};
use serde_json::json;
use uuid::Uuid;

fn report(env: &TestEnv) -> SloReport {
    let pool = env.identity.pool().clone();
    SloReport {
        cards: Arc::clone(&env.state.cards),
        media: Arc::clone(&env.state.media),
        queue: Arc::new(JobQueue::new(pool.clone(), env.clock.clone())),
        notifier: Arc::new(notifications::Notifier::new(
            pool,
            env.clock.clone(),
            Arc::clone(&env.state.push_tokens),
            Arc::new(notifications::FakePushSender::default()),
            notifications::NotifySettings::default(),
        )),
        tokens: Arc::clone(&env.state.push_tokens),
    }
}

#[tokio::test]
async fn measures_send_to_delivered_latency() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let friend = adult(&env).await;
    make_friends(&env, owner.id, friend.id).await;
    let (render, mask) = clean_media(&env, &owner).await;
    // A window no other test writes to: far in this test's own clock future.
    let jump = 400 + i64::from(Uuid::now_v7().as_bytes()[15]);
    env.clock.advance(time::Duration::days(jump));
    let owner = signed_in_again(&env, &owner).await;
    let since = env.clock.now();
    let body = card_body(
        render,
        Some(mask),
        json!({ "mode": "subset", "recipient_ids": [friend.id] }),
    );
    let response = post_card(&env, &owner, &body).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let card = Uuid::parse_str(json_body(response).await["card"]["id"].as_str().unwrap()).unwrap();
    env.clock.advance(time::Duration::seconds(7));
    env.state
        .cards
        .evaluate(&env.state.media, card)
        .await
        .unwrap();

    let snapshot = report(&env).snapshot(since).await.unwrap();
    assert!(snapshot.delivery.count >= 1);
    assert!(
        (snapshot.delivery.p50_secs - 7.0).abs() < 0.5,
        "{:?}",
        snapshot.delivery
    );
    assert!(snapshot.delivery.p95_secs >= 6.5, "{:?}", snapshot.delivery);
    assert!(snapshot.open_holds >= 0);
    assert_eq!(snapshot.lanes.len(), 3);
    assert!(
        !snapshot
            .alerts
            .iter()
            .any(|a| matches!(a, Alert::DeliveryP95 { .. }))
    );
}

#[tokio::test]
async fn an_old_queued_job_trips_the_queue_age_alert() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let r = report(&env);
    // A job due 10 minutes ago on the bulk lane (synthetic backlog), removed afterwards.
    let kind = format!("test.synthetic_backlog.{}", Uuid::now_v7().simple());
    let job = platform::jobs::NewJob::new(Lane::Bulk, kind.clone(), json!({}))
        .run_at(env.clock.now() - time::Duration::minutes(10));
    platform::jobs::enqueue(env.identity.pool(), &job, env.clock.now())
        .await
        .unwrap();
    let snapshot = r.snapshot(env.clock.now()).await;
    sqlx::query("DELETE FROM platform.jobs WHERE kind = $1")
        .bind(&kind)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let snapshot = snapshot.unwrap();
    assert!(
        snapshot.alerts.iter().any(|a| matches!(
            a,
            Alert::QueueAge { lane: Lane::Bulk, oldest_age_secs } if *oldest_age_secs >= 600
        )),
        "{:?}",
        snapshot.alerts
    );
}
