//! T-87 (US-31 AC-31.1): per-type toggles and quiet hours. Each caller only ever reads and writes
//! their own preferences; a type turned off sends nothing; during quiet hours alerts are held
//! and sent when the window ends.

use std::sync::Arc;

use crate::common::*;
use crate::push_tokens::{apns_token, on_device, put};
use crate::social_support::*;
use http::StatusCode;
use notifications::{
    FakePushSender, Notification, Notifier, NotifySettings, Outcome, PushKind, Recipient,
};
use platform::clock::Clock;
use serde_json::{Value, json};
use uuid::Uuid;

const PATH: &str = "/v1/me/notification-preferences";

fn prefs(start: &str, end: &str, zone: Value) -> Value {
    json!({
        "types": { "card_new": true, "reaction": false, "friend": true, "card_review": true },
        "quiet_hours": { "enabled": true, "start": start, "end": end },
        "time_zone": zone,
    })
}

#[tokio::test]
async fn defaults_then_my_own_saved_preferences() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let other = adult(&env).await;
    let defaults = json_body(call(&env, "GET", PATH, &me, None).await).await;
    assert_eq!(
        defaults,
        json!({
            "types": { "card_new": true, "reaction": true, "friend": true, "card_review": true },
            "quiet_hours": { "enabled": true, "start": "22:00", "end": "08:00" },
            "time_zone": null,
        })
    );
    let mine = prefs("23:30", "07:15", json!("Europe/Paris"));
    let response = call(&env, "PUT", PATH, &me, Some(&mine)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, mine);
    assert_eq!(
        json_body(call(&env, "GET", PATH, &me, None).await).await,
        mine
    );
    // Nobody else's preferences change or show up.
    assert_eq!(
        json_body(call(&env, "GET", PATH, &other, None).await).await,
        defaults
    );
}

#[tokio::test]
async fn bad_bodies_are_400() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let mut with_user = prefs("22:00", "08:00", Value::Null);
    with_user["user_id"] = json!(Uuid::now_v7());
    for body in [
        prefs("24:00", "08:00", Value::Null),
        prefs("22:00", "8:00", Value::Null),
        prefs("22:00", "22:00", Value::Null),
        prefs("22:00", "08:00", json!("Mars/Olympus_Mons")),
        prefs("22:00", "08:00", json!("../etc/passwd")),
        with_user,
        json!({ "types": {} }),
    ] {
        let response = call(&env, "PUT", PATH, &me, Some(&body)).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
}

fn notifier(env: &TestEnv, sender: &Arc<FakePushSender>) -> Notifier {
    Notifier::new(
        env.identity.pool().clone(),
        env.clock.clone(),
        Arc::clone(&env.state.push_tokens),
        sender.clone(),
        NotifySettings::default(),
    )
}

fn event_id() -> i64 {
    i64::from_le_bytes(Uuid::now_v7().as_bytes()[8..16].try_into().unwrap()).abs()
}

#[tokio::test]
async fn toggled_off_types_are_not_pushed_and_quiet_hours_hold_until_the_end() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sender = Arc::new(FakePushSender::default());
    let n = notifier(&env, &sender);
    let (me, device, _) = on_device(&env, &adult(&env).await).await;
    let body = json!({ "provider": "apns", "kind": "alert", "token": apns_token() });
    assert_eq!(put(&env, &me, device, &body).await, StatusCode::NO_CONTENT);

    // A quiet window around "now" in UTC: [this hour, this hour + 2).
    let now = env.clock.now();
    let start = now.hour();
    let end = (start + 2) % 24;
    let body = prefs(
        &format!("{start:02}:00"),
        &format!("{end:02}:00"),
        json!("UTC"),
    );
    let response = call(&env, "PUT", PATH, &me, Some(&body)).await;
    assert_eq!(response.status(), StatusCode::OK);

    let reaction = Recipient {
        account_id: me.id,
        notification: Notification::about(PushKind::ReactionNew, Uuid::now_v7()),
    };
    assert_eq!(
        n.notify(event_id(), &reaction).await.unwrap(),
        Outcome::Disabled
    );

    let card = Recipient {
        account_id: me.id,
        notification: Notification::about(PushKind::CardNew, Uuid::now_v7()),
    };
    let id = event_id();
    let Outcome::Held { until } = n.notify(id, &card).await.unwrap() else {
        panic!("expected a held push");
    };
    assert_eq!(until.hour(), end);
    assert_eq!((until.minute(), until.second()), (0, 0));
    assert!(until > now && until - now <= time::Duration::hours(2));
    assert!(sender.sent().is_empty());

    // The deferred job exists once, at the end of the window.
    n.defer(id, &card, until).await.unwrap();
    n.defer(id, &card, until).await.unwrap();
    let jobs: Vec<time::OffsetDateTime> = sqlx::query_scalar(
        "SELECT run_at FROM platform.jobs WHERE kind = 'notify.push' AND unique_key = $1",
    )
    .bind(format!("notify.deferred:{id}:{}:card.new", me.id))
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(jobs, vec![until]);

    // When the window ends the held push goes out.
    env.clock.set(until);
    assert_eq!(n.notify(id, &card).await.unwrap(), Outcome::Sent);
    assert_eq!(sender.sent().len(), 1);
}
