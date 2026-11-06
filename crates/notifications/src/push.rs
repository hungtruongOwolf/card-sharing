//! Content-free push notifications (T-86, US-31 AC-31.2, SR-15): what a push says and the port
//! that delivers it.
//!
//! A payload carries a type, localisation keys the app resolves on the device, and at most one
//! opaque id (or a count for the batched "new cards" push). Never a name, caption, image, URL or
//! any other user content, so the locked-screen preview setting cannot leak anything: every push
//! is the "locked-preview-hidden" variant. The real APNs/FCM adapters are a follow-up (no
//! credentials yet); [`FakePushSender`] stands in outside production.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use serde_json::{Value, json};
use uuid::Uuid;

use crate::tokens::{Provider, PushTarget};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What a push is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PushKind {
    /// A friend's card was delivered to me (`card_id`).
    CardNew,
    /// More than the batch threshold of new cards inside the window (count only).
    CardsNewMany,
    /// Someone reacted to my card (`card_id`).
    ReactionNew,
    /// Someone sent me a friend request (`request_id`).
    FriendRequest,
    /// My friend request was accepted (`user_id` = the new friend).
    FriendAccepted,
    /// My card is held for review (`card_id`, generic copy).
    CardHeld,
    /// Silent: the widget should call `GET /v1/widget` again (widget tokens only).
    WidgetRefresh,
}

impl PushKind {
    pub const ALERTS: [Self; 6] = [
        Self::CardNew,
        Self::CardsNewMany,
        Self::ReactionNew,
        Self::FriendRequest,
        Self::FriendAccepted,
        Self::CardHeld,
    ];

    /// The `t` field of every payload.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CardNew => "card.new",
            Self::CardsNewMany => "cards.new_many",
            Self::ReactionNew => "reaction.new",
            Self::FriendRequest => "friend.request",
            Self::FriendAccepted => "friend.accepted",
            Self::CardHeld => "card.held",
            Self::WidgetRefresh => "widget.refresh",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALERTS
            .into_iter()
            .chain([Self::WidgetRefresh])
            .find(|k| k.as_str() == value)
    }

    /// The name of the opaque id the payload carries, if any.
    fn subject_key(self) -> Option<&'static str> {
        match self {
            Self::CardNew | Self::ReactionNew | Self::CardHeld => Some("card_id"),
            Self::FriendRequest => Some("request_id"),
            Self::FriendAccepted => Some("user_id"),
            Self::CardsNewMany | Self::WidgetRefresh => None,
        }
    }

    /// Localisation key stem (`push.<stem>.title` / `.body` on iOS, `push_<stem>_title` on Android).
    fn loc_stem(self) -> &'static str {
        match self {
            Self::CardNew => "card_new",
            Self::CardsNewMany => "cards_new_many",
            Self::ReactionNew => "reaction_new",
            Self::FriendRequest => "friend_request",
            Self::FriendAccepted => "friend_accepted",
            Self::CardHeld => "card_held",
            Self::WidgetRefresh => "widget_refresh",
        }
    }

    /// iOS notification grouping.
    fn thread(self) -> &'static str {
        match self {
            Self::CardNew | Self::CardsNewMany | Self::CardHeld => "cards",
            Self::ReactionNew => "reactions",
            Self::FriendRequest | Self::FriendAccepted => "friends",
            Self::WidgetRefresh => "widget",
        }
    }

    /// Whether the push counts toward the daily cap (AC-31.4: friend requests do not).
    pub fn counts_toward_cap(self) -> bool {
        !matches!(self, Self::FriendRequest | Self::WidgetRefresh)
    }
}

/// One push, provider-neutral. Build it with the constructors; nothing else can be attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    kind: PushKind,
    subject: Option<Uuid>,
    count: Option<u32>,
}

impl Notification {
    /// An alert about `subject` (the id named by the kind). `CardsNewMany` and `WidgetRefresh`
    /// take no id: use [`Notification::cards_new_many`] / [`Notification::widget_refresh`].
    pub fn about(kind: PushKind, subject: Uuid) -> Self {
        Self {
            kind,
            subject: kind.subject_key().map(|_| subject),
            count: None,
        }
    }

    pub fn cards_new_many(count: u32) -> Self {
        Self {
            kind: PushKind::CardsNewMany,
            subject: None,
            count: Some(count),
        }
    }

    pub fn widget_refresh() -> Self {
        Self {
            kind: PushKind::WidgetRefresh,
            subject: None,
            count: None,
        }
    }

    pub fn kind(&self) -> PushKind {
        self.kind
    }

    /// The opaque id the push is about, if its kind carries one.
    pub fn subject(&self) -> Option<Uuid> {
        self.subject
    }

    /// The custom data both providers carry: `t` plus the one opaque id.
    fn data(&self) -> serde_json::Map<String, Value> {
        let mut data = serde_json::Map::new();
        data.insert("t".to_owned(), json!(self.kind.as_str()));
        if let (Some(key), Some(id)) = (self.kind.subject_key(), self.subject) {
            data.insert(key.to_owned(), json!(id.to_string()));
        }
        data
    }

    /// The APNs JSON body.
    pub fn apns_payload(&self) -> Value {
        if self.kind == PushKind::WidgetRefresh {
            // WidgetKit push: no alert, no content; the widget reloads its timeline.
            return json!({ "aps": { "content-changed": true } });
        }
        let stem = self.kind.loc_stem();
        let mut alert = json!({
            "title-loc-key": format!("push.{stem}.title"),
            "loc-key": format!("push.{stem}.body"),
        });
        if let Some(count) = self.count {
            alert["loc-args"] = json!([count.to_string()]);
        }
        let mut body = Value::Object(self.data());
        body["aps"] = json!({
            "alert": alert,
            "thread-id": self.kind.thread(),
            "sound": "default",
        });
        body
    }

    /// The FCM HTTP v1 `message` (without `token`). Data values are strings, as FCM requires.
    pub fn fcm_message(&self) -> Value {
        if self.kind == PushKind::WidgetRefresh {
            return json!({
                "data": self.data(),
                "android": { "priority": "normal" },
            });
        }
        let stem = self.kind.loc_stem();
        let mut notification = json!({
            "title_loc_key": format!("push_{stem}_title"),
            "body_loc_key": format!("push_{stem}_body"),
            "tag": self.kind.thread(),
        });
        if let Some(count) = self.count {
            notification["body_loc_args"] = json!([count.to_string()]);
        }
        json!({
            "data": self.data(),
            "android": { "priority": "high", "notification": notification },
        })
    }

    /// The body for `provider`.
    pub fn payload_for(&self, provider: Provider) -> Value {
        match provider {
            Provider::Apns => self.apns_payload(),
            Provider::Fcm => self.fcm_message(),
        }
    }
}

/// What the provider said about one push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Sent,
    /// The token is unregistered or invalid: prune it.
    InvalidToken,
    /// No adapter configured: nothing was sent.
    NotConfigured,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("push provider error: {0}")]
pub struct PushError(pub String);

/// The delivery port (APNs / FCM adapters, the fake, or nothing).
pub trait PushSender: Send + Sync {
    fn send<'a>(
        &'a self,
        target: &'a PushTarget,
        notification: &'a Notification,
    ) -> BoxFuture<'a, Result<Delivery, PushError>>;
}

/// No credentials configured: every push is dropped (the activity tab still has the event).
#[derive(Debug, Default, Clone, Copy)]
pub struct UnconfiguredPushSender;

impl PushSender for UnconfiguredPushSender {
    fn send<'a>(
        &'a self,
        _target: &'a PushTarget,
        _notification: &'a Notification,
    ) -> BoxFuture<'a, Result<Delivery, PushError>> {
        Box::pin(async { Ok(Delivery::NotConfigured) })
    }
}

/// FCM tokens starting with this prefix (or APNs tokens starting with `dead`) are reported as
/// invalid by [`FakePushSender`].
pub const FAKE_INVALID_FCM_PREFIX: &str = "invalid";
const FAKE_INVALID_APNS_PREFIX: &str = "dead";

/// A push the fake "delivered".
#[derive(Debug, Clone, PartialEq)]
pub struct SentPush {
    pub token_id: Uuid,
    pub provider: Provider,
    pub kind: PushKind,
    pub payload: Value,
}

/// Development/test adapter: records payloads in memory and logs the type only. Refused when
/// `API_ENV=production` (like the content-safety fakes).
#[derive(Debug, Default)]
pub struct FakePushSender {
    sent: Mutex<Vec<SentPush>>,
}

impl FakePushSender {
    fn sent_guard(&self) -> std::sync::MutexGuard<'_, Vec<SentPush>> {
        self.sent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Everything "delivered" so far.
    pub fn sent(&self) -> Vec<SentPush> {
        self.sent_guard().clone()
    }
}

impl PushSender for FakePushSender {
    fn send<'a>(
        &'a self,
        target: &'a PushTarget,
        notification: &'a Notification,
    ) -> BoxFuture<'a, Result<Delivery, PushError>> {
        let token = target.token.expose();
        let invalid = match target.provider {
            Provider::Apns => token.starts_with(FAKE_INVALID_APNS_PREFIX),
            Provider::Fcm => token.starts_with(FAKE_INVALID_FCM_PREFIX),
        };
        let delivery = if invalid {
            Delivery::InvalidToken
        } else {
            self.sent_guard().push(SentPush {
                token_id: target.id,
                provider: target.provider,
                kind: notification.kind(),
                payload: notification.payload_for(target.provider),
            });
            Delivery::Sent
        };
        tracing::info!(
            event = "push.fake_sent",
            kind = notification.kind().as_str(),
            provider = target.provider.as_str(),
            ?delivery
        );
        Box::pin(async move { Ok(delivery) })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;

    const ID: &str = "0192f0c1-7c1e-7a3b-9d2e-123456789abc";

    fn id() -> Uuid {
        Uuid::parse_str(ID).unwrap()
    }

    /// Every string key and value in a payload, recursively.
    fn strings(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    out.push(k.clone());
                    strings(v, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
            Value::String(s) => out.push(s.clone()),
            _ => {}
        }
    }

    fn all_payloads() -> Vec<Value> {
        let mut notes: Vec<Notification> = PushKind::ALERTS
            .into_iter()
            .filter(|k| *k != PushKind::CardsNewMany)
            .map(|k| Notification::about(k, id()))
            .collect();
        notes.push(Notification::cards_new_many(7));
        notes.push(Notification::widget_refresh());
        notes
            .iter()
            .flat_map(|n| [n.apns_payload(), n.fcm_message()])
            .collect()
    }

    /// AC-31.2: no URL, caption, image, name or other content field in any payload.
    #[test]
    fn payloads_carry_no_content() {
        for payload in all_payloads() {
            let mut found = Vec::new();
            strings(&payload, &mut found);
            for s in &found {
                let lower = s.to_ascii_lowercase();
                for banned in [
                    "http",
                    "url",
                    "caption",
                    "image",
                    "media",
                    "name",
                    "mutable-content",
                    "attachment",
                ] {
                    assert!(!lower.contains(banned), "{banned} in {payload}");
                }
                // Only loc keys, kinds, threads, priorities, a uuid or a small count.
                let allowed = s.starts_with("push.")
                    || s.starts_with("push_")
                    || Uuid::parse_str(s).is_ok()
                    || s.parse::<u32>().is_ok()
                    || PushKind::parse(s).is_some()
                    || [
                        "aps",
                        "alert",
                        "t",
                        "card_id",
                        "request_id",
                        "user_id",
                        "thread-id",
                        "sound",
                        "default",
                        "loc-key",
                        "title-loc-key",
                        "loc-args",
                        "data",
                        "android",
                        "priority",
                        "high",
                        "normal",
                        "notification",
                        "tag",
                        "title_loc_key",
                        "body_loc_key",
                        "body_loc_args",
                        "cards",
                        "reactions",
                        "friends",
                        "content-changed",
                    ]
                    .contains(&s.as_str());
                assert!(allowed, "unexpected string {s:?} in {payload}");
            }
        }
    }

    #[test]
    fn apns_snapshot() {
        assert_eq!(
            Notification::about(PushKind::CardNew, id()).apns_payload(),
            json!({
                "aps": {
                    "alert": { "title-loc-key": "push.card_new.title", "loc-key": "push.card_new.body" },
                    "thread-id": "cards",
                    "sound": "default"
                },
                "t": "card.new",
                "card_id": ID
            })
        );
        assert_eq!(
            Notification::cards_new_many(5).apns_payload()["aps"]["alert"]["loc-args"],
            json!(["5"])
        );
        assert_eq!(
            Notification::widget_refresh().apns_payload(),
            json!({ "aps": { "content-changed": true } })
        );
    }

    #[test]
    fn fcm_snapshot() {
        assert_eq!(
            Notification::about(PushKind::FriendAccepted, id()).fcm_message(),
            json!({
                "data": { "t": "friend.accepted", "user_id": ID },
                "android": {
                    "priority": "high",
                    "notification": {
                        "title_loc_key": "push_friend_accepted_title",
                        "body_loc_key": "push_friend_accepted_body",
                        "tag": "friends"
                    }
                }
            })
        );
        assert_eq!(
            Notification::widget_refresh().fcm_message(),
            json!({ "data": { "t": "widget.refresh" }, "android": { "priority": "normal" } })
        );
    }

    #[test]
    fn kinds_round_trip_and_cap_rules() {
        for kind in PushKind::ALERTS
            .into_iter()
            .chain([PushKind::WidgetRefresh])
        {
            assert_eq!(PushKind::parse(kind.as_str()), Some(kind));
        }
        assert!(!PushKind::FriendRequest.counts_toward_cap());
        assert!(!PushKind::WidgetRefresh.counts_toward_cap());
        assert!(PushKind::CardNew.counts_toward_cap());
        // An id is dropped for kinds that carry none.
        assert_eq!(
            Notification::about(PushKind::CardsNewMany, id()),
            Notification {
                kind: PushKind::CardsNewMany,
                subject: None,
                count: None
            }
        );
    }
}
