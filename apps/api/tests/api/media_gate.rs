//! T-72: the known-CSAM hash gate fails closed. Clean media moves to the clean bucket; a match
//! blocks it, preserves evidence, opens an incident and suspends the account; an unavailable
//! matcher leaves the media `scanning` (retried, never delivered).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::common::*;
use crate::media_process::{real_png, real_png_with, uploaded_id};
use crate::social_support::*;
use api::workers::{CsamSuspension, SUSPEND_ACCOUNT_JOB};
use http::StatusCode;
use image::Rgba;
use media::safety::{BoxFuture, FAKE_MATCH_MARKER};
use media::{
    Bucket, ContentGates, FakeClassifier, FakeHashMatcher, HashMatcher, HashVerdict,
    ProcessSettings, ScanInput, Unavailable,
};
use platform::clock::Clock;
use platform::jobs::JobError;
use platform::outbox::{StoredEvent, Subscriber};
use uuid::Uuid;

pub fn gates(matcher: Arc<dyn HashMatcher>) -> ProcessSettings {
    ProcessSettings {
        decode_timeout: Duration::from_secs(20),
        gates: ContentGates {
            matcher,
            classifier: Arc::new(FakeClassifier),
            ..ContentGates::unconfigured()
        },
    }
}

pub fn fake_gates() -> ProcessSettings {
    gates(Arc::new(FakeHashMatcher))
}

fn magenta() -> Rgba<u8> {
    Rgba(FAKE_MATCH_MARKER)
}

async fn state(env: &TestEnv, id: Uuid) -> (String, String) {
    sqlx::query_as("SELECT state, bucket FROM media.media_objects WHERE id = $1")
        .bind(id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn clean_media_moves_to_the_clean_bucket() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let quarantine = env.media_store.count(Bucket::Quarantine);
    let clean = env.media_store.count(Bucket::Clean);
    let id = uploaded_id(&env, &user, "card_render", real_png(800, 600)).await;
    env.state
        .media
        .process_one(id, &fake_gates())
        .await
        .unwrap();

    assert_eq!(
        state(&env, id).await,
        ("clean".to_owned(), "clean".to_owned())
    );
    // Re-encode + 3 derivatives served from clean; quarantine copies and raw upload gone.
    assert_eq!(env.media_store.count(Bucket::Clean), clean + 4);
    assert_eq!(env.media_store.count(Bucket::Quarantine), quarantine);
    let (provider, raw): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT scan_provider, raw_storage_key FROM media.media_objects WHERE id = $1",
    )
    .bind(id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(provider.as_deref(), Some("fake"));
    assert_eq!(raw, None);
    let quarantined: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM media.media_derivatives WHERE media_id = $1 AND bucket <> 'clean'",
    )
    .bind(id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(quarantined, 0);
}

#[tokio::test]
async fn a_match_blocks_preserves_evidence_and_suspends_the_account() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let quarantine = env.media_store.count(Bucket::Quarantine);
    let clean = env.media_store.count(Bucket::Clean);
    let legal = env.media_store.count(Bucket::LegalHold);
    let id = uploaded_id(&env, &user, "card_render", real_png_with(64, 64, magenta())).await;
    env.state
        .media
        .process_one(id, &fake_gates())
        .await
        .unwrap();

    assert_eq!(
        state(&env, id).await,
        ("blocked".to_owned(), "legal_hold".to_owned())
    );
    assert_eq!(env.media_store.count(Bucket::LegalHold), legal + 1);
    assert_eq!(
        env.media_store.count(Bucket::Clean),
        clean,
        "nothing servable"
    );
    assert_eq!(env.media_store.count(Bucket::Quarantine), quarantine);
    let (account, category, evidence): (Uuid, String, Option<String>) = sqlx::query_as(
        "SELECT account_id, category, evidence_key FROM moderation.csam_incidents WHERE media_id = $1",
    )
    .bind(id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!((account, category.as_str()), (user.id, "test"));
    assert!(evidence.is_some());

    // The owner sees the state only; no URL anywhere.
    let response = call(&env, "GET", &format!("/v1/media/{id}"), &user, None).await;
    let body = json_body(response).await;
    assert_eq!(body["media"]["state"], "blocked");
    assert!(!body.to_string().contains("url"));

    // The outbox event becomes the suspension job; the job suspends and revokes sessions.
    let (event_id, payload): (i64, serde_json::Value) = sqlx::query_as(
        "SELECT id, payload FROM platform.outbox_events
         WHERE topic = 'media.csam_matched' AND payload->>'media_id' = $1",
    )
    .bind(id.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    let job = CsamSuspension
        .job(&StoredEvent {
            id: event_id,
            topic: "media.csam_matched".to_owned(),
            payload: payload.clone(),
            created_at: env.clock.now(),
        })
        .unwrap();
    assert_eq!(job.kind, SUSPEND_ACCOUNT_JOB);
    assert_eq!(job.payload["account_id"], user.id.to_string());
    assert!(!payload.to_string().contains("url"));
    assert!(
        env.identity
            .suspend_account(user.id, "csam_match")
            .await
            .unwrap()
    );
    let status: String = sqlx::query_scalar("SELECT status FROM identity.accounts WHERE id = $1")
        .bind(user.id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(status, "suspended");
    let after = call(&env, "GET", &format!("/v1/media/{id}"), &user, None).await;
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);
    // Idempotent.
    assert!(
        !env.identity
            .suspend_account(user.id, "csam_match")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn masks_and_originals_are_matched_too() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    for kind in ["mask", "original", "avatar"] {
        let id = uploaded_id(&env, &user, kind, real_png_with(32, 32, magenta())).await;
        env.state
            .media
            .process_one(id, &fake_gates())
            .await
            .unwrap();
        assert_eq!(state(&env, id).await.0, "blocked", "{kind}");
    }
}

/// A matcher that is down until told otherwise.
struct Flaky(AtomicBool);

impl HashMatcher for Flaky {
    fn check<'a>(
        &'a self,
        _: &'a ScanInput<'a>,
    ) -> BoxFuture<'a, Result<HashVerdict, Unavailable>> {
        let up = self.0.load(Ordering::SeqCst);
        Box::pin(async move {
            if up {
                Ok(HashVerdict::NoMatch {
                    provider: "flaky".to_owned(),
                    list_version: "1".to_owned(),
                })
            } else {
                Err(Unavailable("down".to_owned()))
            }
        })
    }
}

#[tokio::test]
async fn matcher_outage_keeps_media_scanning_until_it_recovers() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let clean = env.media_store.count(Bucket::Clean);
    let flaky = Arc::new(Flaky(AtomicBool::new(false)));
    let settings = gates(flaky.clone());
    let id = uploaded_id(&env, &user, "card_render", real_png(128, 128)).await;

    for _ in 0..2 {
        let result = env.state.media.process_one(id, &settings).await;
        assert!(matches!(result, Err(JobError::Retry(_))), "{result:?}");
        assert_eq!(state(&env, id).await.0, "scanning");
        assert_eq!(env.media_store.count(Bucket::Clean), clean);
    }
    // Unconfigured gates (the production default until a vendor exists) behave the same.
    let result = env
        .state
        .media
        .process_one(id, &ProcessSettings::default())
        .await;
    assert!(matches!(result, Err(JobError::Retry(_))));

    flaky.0.store(true, Ordering::SeqCst);
    env.state.media.process_one(id, &settings).await.unwrap();
    assert_eq!(state(&env, id).await.0, "clean");
    assert_eq!(env.media_store.count(Bucket::Clean), clean + 4);
}

#[tokio::test]
async fn scan_backlog_reports_the_oldest_pending_upload() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    uploaded_id(&env, &user, "card_render", real_png(16, 16)).await;
    env.clock.advance(time::Duration::minutes(6));
    let (pending, age) = env.state.media.scan_backlog().await.unwrap();
    assert!(pending >= 1);
    assert!(age > api::workers::SCAN_BACKLOG_ALERT_SECS, "{age}");
}
