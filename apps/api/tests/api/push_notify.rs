//! T-86 (US-31, AC-31.2, AC-31.4, SR-15): outbox events become content-free pushes to the
//! recipient's live tokens; new cards batch after 3 in 10 minutes; 30 alerts a day (friend
//! requests excluded); a token the provider rejects is pruned; a re-run job pushes nothing twice.

use std::sync::Arc;

use crate::common::*;
use crate::push_tokens::{apns_token, on_device, put};
use crate::social_support::*;
use http::StatusCode;
use notifications::{
    FAKE_INVALID_FCM_PREFIX, FakePushSender, Notification, Notifier, NotifySettings, Outcome,
    PushKind, Recipient, TokenKind,
};
use serde_json::{Value, json};
use uuid::Uuid;

struct Push {
    sender: Arc<FakePushSender>,
    notifier: Notifier,
}

fn push(env: &TestEnv) -> Push {
    let sender = Arc::new(FakePushSender::default());
    let notifier = Notifier::new(
        env.identity.pool().clone(),
        env.clock.clone(),
        Arc::clone(&env.state.push_tokens),
        sender.clone(),
        NotifySettings::default(),
    );
    Push { sender, notifier }
}

/// A user with an alert and a widget token on one device.
async fn with_tokens(env: &TestEnv) -> User {
    let (user, device, _) = on_device(env, &adult(env).await).await;
    for kind in ["alert", "widget"] {
        let body = json!({ "provider": "apns", "kind": kind, "token": apns_token() });
        assert_eq!(put(env, &user, device, &body).await, StatusCode::NO_CONTENT);
    }
    user
}

fn event_id() -> i64 {
    // Unique per call across tests (outbox ids are BIGINT identities; tests use a wide range).
    i64::from_le_bytes(Uuid::now_v7().as_bytes()[8..16].try_into().unwrap()).abs()
}

fn alert(account_id: Uuid, kind: PushKind) -> Recipient {
    Recipient {
        account_id,
        notification: Notification::about(kind, Uuid::now_v7()),
    }
}

#[tokio::test]
async fn delivered_card_pushes_an_alert_and_a_widget_refresh_without_content() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let p = push(&env);
    let me = with_tokens(&env).await;
    let card = Uuid::now_v7();
    let id = event_id();
    for recipient in notifications::recipients_of(
        "cards.delivered",
        &json!({ "card_id": card, "owner_id": Uuid::now_v7(), "recipient_ids": [me.id] }),
    ) {
        p.notifier.notify(id, &recipient).await.unwrap();
    }
    let sent = p.sender.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].kind, PushKind::CardNew);
    assert_eq!(sent[0].payload["card_id"], json!(card.to_string()));
    assert_eq!(sent[1].kind, PushKind::WidgetRefresh);
    // No name, caption or URL anywhere (the sender's name is not even known to the notifier).
    let text: String = sent.iter().map(|s| s.payload.to_string()).collect();
    for banned in ["http", "caption", "image", &me.name, &me.handle] {
        assert!(!text.contains(banned), "{banned}");
    }
    // The same event again (a re-run job): no second alert.
    let again = Recipient {
        account_id: me.id,
        notification: Notification::about(PushKind::CardNew, card),
    };
    assert_eq!(
        p.notifier.notify(id, &again).await.unwrap(),
        Outcome::Duplicate
    );
}

#[tokio::test]
async fn new_cards_batch_after_three_in_ten_minutes() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let p = push(&env);
    let me = with_tokens(&env).await;
    let mut outcomes = Vec::new();
    for _ in 0..6 {
        outcomes.push(
            p.notifier
                .notify(event_id(), &alert(me.id, PushKind::CardNew))
                .await
                .unwrap(),
        );
    }
    assert_eq!(
        outcomes,
        vec![
            Outcome::Sent,
            Outcome::Sent,
            Outcome::Sent,
            Outcome::SentBatch,
            Outcome::Batched,
            Outcome::Batched
        ]
    );
    let sent = p.sender.sent();
    assert_eq!(sent.len(), 4);
    assert_eq!(sent[3].kind, PushKind::CardsNewMany);
    assert_eq!(sent[3].payload["aps"]["alert"]["loc-args"], json!(["4"]));
    assert!(sent[3].payload.get("card_id").is_none());

    // After the window, single pushes again.
    env.clock.advance(time::Duration::minutes(11));
    assert_eq!(
        p.notifier
            .notify(event_id(), &alert(me.id, PushKind::CardNew))
            .await
            .unwrap(),
        Outcome::Sent
    );
}

#[tokio::test]
async fn thirty_alerts_a_day_but_friend_requests_always_go_through() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let p = push(&env);
    let me = with_tokens(&env).await;
    for _ in 0..30 {
        assert_eq!(
            p.notifier
                .notify(event_id(), &alert(me.id, PushKind::ReactionNew))
                .await
                .unwrap(),
            Outcome::Sent
        );
    }
    assert_eq!(
        p.notifier
            .notify(event_id(), &alert(me.id, PushKind::ReactionNew))
            .await
            .unwrap(),
        Outcome::Capped
    );
    assert_eq!(
        p.notifier
            .notify(event_id(), &alert(me.id, PushKind::FriendRequest))
            .await
            .unwrap(),
        Outcome::Sent
    );
    // A rolling day later the cap has room again.
    env.clock.advance(time::Duration::hours(25));
    assert_eq!(
        p.notifier
            .notify(event_id(), &alert(me.id, PushKind::CardHeld))
            .await
            .unwrap(),
        Outcome::Sent
    );
}

#[tokio::test]
async fn rejected_tokens_are_pruned_and_logged_out_devices_get_nothing() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let p = push(&env);
    let (me, device, session) = on_device(&env, &adult(&env).await).await;
    let body = json!({
        "provider": "fcm",
        "kind": "alert",
        "token": format!("{FAKE_INVALID_FCM_PREFIX}-{}", Uuid::now_v7().simple()),
    });
    assert_eq!(put(&env, &me, device, &body).await, StatusCode::NO_CONTENT);
    assert_eq!(
        p.notifier
            .notify(event_id(), &alert(me.id, PushKind::FriendAccepted))
            .await
            .unwrap(),
        Outcome::Failed
    );
    assert!(
        env.state
            .push_tokens
            .live_targets(me.id, TokenKind::Alert)
            .await
            .unwrap()
            .is_empty()
    );

    // A valid token, then logout: nothing more is sent.
    let body = json!({ "provider": "apns", "kind": "alert", "token": apns_token() });
    assert_eq!(put(&env, &me, device, &body).await, StatusCode::NO_CONTENT);
    env.identity
        .revoke_session(session, identity::RevokeReason::Logout)
        .await
        .unwrap();
    assert_eq!(
        p.notifier
            .notify(event_id(), &alert(me.id, PushKind::ReactionNew))
            .await
            .unwrap(),
        Outcome::NoTarget
    );
    assert!(p.sender.sent().is_empty());
}

#[tokio::test]
async fn unfriend_refreshes_widgets_silently_and_never_alerts() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let p = push(&env);
    let a = with_tokens(&env).await;
    let b = with_tokens(&env).await;
    let payload: Value = json!({ "actor_id": a.id, "other_id": b.id });
    for recipient in notifications::recipients_of("social.friendship_removed", &payload) {
        p.notifier.notify(event_id(), &recipient).await.unwrap();
    }
    let sent = p.sender.sent();
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().all(|s| s.kind == PushKind::WidgetRefresh));
    assert!(
        sent.iter()
            .all(|s| s.payload == json!({ "aps": { "content-changed": true } }))
    );
}
