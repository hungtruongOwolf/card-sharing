//! #331: account notices for privacy and sharing events: a requested export (SR-36: every device
//! is told, whatever the toggles and quiet hours), a ready export and an automatically disabled
//! public link reach the owner as pushes and activity items, with ids only.

use std::sync::Arc;

use crate::common::*;
use crate::push_tokens::{apns_token, on_device, put};
use crate::social_support::*;
use http::StatusCode;
use notifications::{FakePushSender, Notifier, NotifySettings, Outcome, PushKind};
use serde_json::{Value, json};
use uuid::Uuid;

fn notifier(env: &TestEnv) -> (Arc<FakePushSender>, Notifier) {
    let sender = Arc::new(FakePushSender::default());
    let notifier = Notifier::new(
        env.identity.pool().clone(),
        env.clock.clone(),
        Arc::clone(&env.state.push_tokens),
        sender.clone(),
        NotifySettings::default(),
    );
    (sender, notifier)
}

fn event_id() -> i64 {
    i64::from_le_bytes(Uuid::now_v7().as_bytes()[8..16].try_into().unwrap()).abs()
}

/// A user with alert tokens on two devices.
async fn on_two_devices(env: &TestEnv) -> User {
    let user = adult(env).await;
    let mut last = user.clone();
    for _ in 0..2 {
        let (signed_in, device, _) = on_device(env, &user).await;
        let body = json!({ "provider": "apns", "kind": "alert", "token": apns_token() });
        assert_eq!(
            put(env, &signed_in, device, &body).await,
            StatusCode::NO_CONTENT
        );
        last = signed_in;
    }
    last
}

#[tokio::test]
async fn an_export_request_reaches_every_device_despite_toggles_and_quiet_hours() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (sender, notifier) = notifier(&env);
    let me = on_two_devices(&env).await;
    // Everything switched off and quiet all day.
    let prefs = json!({
        "types": { "card_new": false, "reaction": false, "friend": false, "card_review": false },
        "quiet_hours": { "enabled": true, "start": "00:00", "end": "23:59" },
        "time_zone": "UTC",
    });
    let response = call(
        &env,
        "PUT",
        "/v1/me/notification-preferences",
        &me,
        Some(&prefs),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let export = Uuid::now_v7();
    let recipients = notifications::recipients_of(
        "privacy.export_requested",
        &json!({ "export_id": export, "account_id": me.id }),
    );
    assert_eq!(recipients.len(), 1);
    let outcome = notifier.notify(event_id(), &recipients[0]).await.unwrap();
    assert_eq!(outcome, Outcome::Sent);
    let sent = sender.sent();
    assert_eq!(sent.len(), 2, "one push per device");
    for push in &sent {
        assert_eq!(push.kind, PushKind::ExportRequested);
        assert_eq!(push.payload["export_id"], json!(export.to_string()));
    }
}

#[tokio::test]
async fn export_ready_and_link_disabled_reach_the_owner() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (sender, notifier) = notifier(&env);
    let me = on_two_devices(&env).await;
    let export = Uuid::now_v7();
    let link = Uuid::now_v7();
    for (topic, payload) in [
        (
            "privacy.export_ready",
            json!({ "export_id": export, "account_id": me.id }),
        ),
        (
            "sharing.link_disabled",
            json!({ "link_id": link, "board_id": Uuid::now_v7(), "owner_id": me.id }),
        ),
    ] {
        for recipient in notifications::recipients_of(topic, &payload) {
            assert_eq!(recipient.account_id, me.id);
            notifier.notify(event_id(), &recipient).await.unwrap();
        }
    }
    let kinds: Vec<PushKind> = sender.sent().iter().map(|push| push.kind).collect();
    assert!(kinds.contains(&PushKind::ExportReady));
    assert!(kinds.contains(&PushKind::LinkDisabled));
}

#[tokio::test]
async fn the_three_notices_appear_in_the_activity_tab_with_ids_only() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let export = Uuid::now_v7();
    let link = Uuid::now_v7();
    for (topic, payload) in [
        (
            "privacy.export_requested",
            json!({ "export_id": export, "account_id": me.id }),
        ),
        (
            "privacy.export_ready",
            json!({ "export_id": export, "account_id": me.id }),
        ),
        (
            "sharing.link_disabled",
            json!({ "link_id": link, "board_id": Uuid::now_v7(), "owner_id": me.id }),
        ),
    ] {
        env.state
            .activity
            .record(event_id(), topic, &payload)
            .await
            .unwrap();
    }
    let response = call(&env, "GET", "/v1/activity", &me, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = json_body(response).await;
    let items = body["items"].as_array().unwrap();
    let kinds: Vec<&str> = items.iter().filter_map(|i| i["kind"].as_str()).collect();
    for kind in ["export.requested", "export.ready", "link.disabled"] {
        assert!(kinds.contains(&kind), "{kind} in {kinds:?}");
    }
    // Someone else sees none of them.
    let other = adult(&env).await;
    let response = call(&env, "GET", "/v1/activity", &other, None).await;
    let body: Value = json_body(response).await;
    assert!(body["items"].as_array().unwrap().is_empty());
}
