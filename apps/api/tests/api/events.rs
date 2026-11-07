//! ADR-0010: `GET /v1/events`. Each stream receives only its account's events, as ids; delivery,
//! replies and reactions are announced from their transactions through `NOTIFY`; streams are
//! capped per account and end when the session is revoked. The database (and so the NOTIFY
//! channel) is shared with concurrent tests: every assertion is about this test's own accounts.

use std::time::Duration;

use crate::cards_read::deliver_to;
use crate::cards_send::{clean_media, post_card};
use crate::common::*;
use crate::social_support::*;
use api::realtime::RealtimeSettings;
use axum::body::Body;
use http::{Response, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use uuid::Uuid;

/// A test env whose hub uses `settings` and whose instance listener runs.
async fn env_with(settings: RealtimeSettings) -> Option<TestEnv> {
    let env = TestEnv::with_realtime(settings).await?;
    tokio::spawn(api::realtime::listen(
        env.identity.pool().clone(),
        std::sync::Arc::clone(&env.state.realtime),
    ));
    env.state.realtime.wait_listening().await;
    Some(env)
}

async fn open(env: &TestEnv, user: &User) -> Response<Body> {
    send(&env.router, get_request("/v1/events", Some(&user.token))).await
}

/// One parsed SSE frame (comments such as the heartbeat are skipped).
#[derive(Debug)]
struct Frame {
    event: String,
    data: Value,
}

struct Stream {
    body: Body,
    buffer: String,
}

impl Stream {
    async fn opened(env: &TestEnv, user: &User) -> Self {
        let response = open(env, user).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"].to_str().unwrap(),
            "text/event-stream"
        );
        let mut stream = Self {
            body: response.into_body(),
            buffer: String::new(),
        };
        let ready = stream.next(Duration::from_secs(5)).await.unwrap();
        assert_eq!(ready.event, "ready");
        stream
    }

    /// The next frame within `wait`, or `None`.
    async fn next(&mut self, wait: Duration) -> Option<Frame> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if let Some(end) = self.buffer.find("\n\n") {
                let raw: String = self.buffer.drain(..end + 2).collect();
                let mut event = None;
                let mut data = String::new();
                for line in raw.lines() {
                    if let Some(v) = line.strip_prefix("event:") {
                        event = Some(v.trim().to_owned());
                    } else if let Some(v) = line.strip_prefix("data:") {
                        data.push_str(v.trim());
                    }
                }
                match event {
                    Some(event) => {
                        return Some(Frame {
                            event,
                            data: serde_json::from_str(&data).unwrap_or(Value::Null),
                        });
                    }
                    None => continue,
                }
            }
            let frame = tokio::time::timeout_at(deadline, self.body.frame())
                .await
                .ok()??
                .ok()?;
            if let Ok(bytes) = frame.into_data() {
                self.buffer.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
    }

    /// The next frame about `card`, skipping others (e.g. `resync`), within `wait`.
    async fn about(&mut self, card: Uuid, wait: Duration) -> Option<Frame> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let frame = self.next(left).await?;
            if frame.data["card_id"] == json!(card) {
                return Some(frame);
            }
        }
    }
}

const WAIT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn deliveries_replies_and_reactions_reach_only_their_addressee_as_ids() {
    let Some(env) = env_with(RealtimeSettings::default()).await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let carol = adult(&env).await;
    make_friends(&env, alice.id, bob.id).await;
    make_friends(&env, alice.id, carol.id).await;
    let mut alice_events = Stream::opened(&env, &alice).await;
    let mut bob_events = Stream::opened(&env, &bob).await;
    let mut carol_events = Stream::opened(&env, &carol).await;

    // Alice sends to Bob: Bob hears `card.delivered` with the id only; Carol hears nothing.
    let card = deliver_to(&env, &alice, &[&bob]).await;
    let frame = bob_events.about(card, WAIT).await.unwrap();
    assert_eq!(frame.event, "card.delivered");
    assert_eq!(frame.data, json!({ "card_id": card }));
    assert!(
        carol_events
            .about(card, Duration::from_millis(1500))
            .await
            .is_none()
    );

    // Bob reacts: Alice (the owner) hears `card.reaction`.
    let response = call(
        &env,
        "PUT",
        &format!("/v1/cards/{card}/reaction"),
        &bob,
        Some(&json!({ "kind": "react.heart" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let frame = alice_events.about(card, WAIT).await.unwrap();
    assert_eq!(frame.event, "card.reaction");
    assert_eq!(frame.data, json!({ "card_id": card }));

    // Bob replies: Alice hears `card.reply` with the original's id.
    let (render, mask) = clean_media(&env, &bob).await;
    let body = json!({
        "client_card_id": Uuid::now_v7(),
        "render_media_id": render,
        "mask_media_id": mask,
        "style": crate::cards_send::style(),
        "reply_to_card_id": card,
    });
    let response = post_card(&env, &bob, &body).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let reply = Uuid::parse_str(json_body(response).await["card"]["id"].as_str().unwrap()).unwrap();
    env.state
        .cards
        .evaluate(&env.state.media, reply)
        .await
        .unwrap();
    let frame = alice_events.about(reply, WAIT).await.unwrap();
    assert_eq!(frame.event, "card.reply");
    assert_eq!(frame.data, json!({ "card_id": reply, "reply_to": card }));
}

#[tokio::test]
async fn streams_are_capped_per_account() {
    let Some(env) = env_with(RealtimeSettings {
        per_account_max: 2,
        ..RealtimeSettings::default()
    })
    .await
    else {
        return;
    };
    let alice = adult(&env).await;
    let first = Stream::opened(&env, &alice).await;
    let _second = Stream::opened(&env, &alice).await;
    assert_problem(
        open(&env, &alice).await,
        StatusCode::TOO_MANY_REQUESTS,
        "RATE_LIMITED",
    )
    .await;
    // Closing one frees its slot.
    drop(first);
    let _third = Stream::opened(&env, &alice).await;
    // Other accounts are not affected.
    let bob = adult(&env).await;
    let _bob = Stream::opened(&env, &bob).await;
}

#[tokio::test]
async fn a_revoked_session_ends_the_stream() {
    let Some(env) = env_with(RealtimeSettings {
        session_recheck: Duration::from_millis(300),
        ..RealtimeSettings::default()
    })
    .await
    else {
        return;
    };
    let alice = adult(&env).await;
    let mut stream = Stream::opened(&env, &alice).await;
    let response = call(&env, "POST", "/v1/sessions/revoke-all", &alice, None).await;
    assert!(response.status().is_success(), "{}", response.status());
    let frame = stream.next(WAIT).await.unwrap();
    assert_eq!(frame.event, "reconnect");
    assert_eq!(frame.data["reason"], "session");
    assert!(stream.next(Duration::from_secs(2)).await.is_none());
    // Reconnecting with the revoked token is refused.
    assert_eq!(open(&env, &alice).await.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn guests_cannot_open_a_stream() {
    let Some(env) = env_with(RealtimeSettings::default()).await else {
        return;
    };
    let response = send(&env.router, get_request("/v1/events", None)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
