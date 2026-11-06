//! S3 adapter against a real S3-compatible server (MinIO in CI). Skipped when `TEST_S3_ENDPOINT`
//! is unset, unless `REQUIRE_INTEGRATION_TESTS=1` (CI), where it fails instead.
//!
//! Env: `TEST_S3_ENDPOINT` (e.g. `http://127.0.0.1:9000`), `TEST_S3_ACCESS_KEY_ID`,
//! `TEST_S3_SECRET_ACCESS_KEY`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use bytes::Bytes;
use media::{
    Bucket, DEFAULT_MAX_OBJECT_BYTES, ObjectKey, ObjectStore, S3Buckets, S3ObjectStore, S3Settings,
};
use platform::clock::SystemClock;
use platform::config::Secret;

fn env(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => Some(v),
        _ => {
            assert!(
                std::env::var("REQUIRE_INTEGRATION_TESTS").as_deref() != Ok("1"),
                "{key} must be set when REQUIRE_INTEGRATION_TESTS=1"
            );
            None
        }
    }
}

fn settings(secret: String) -> Option<S3Settings> {
    let endpoint = env("TEST_S3_ENDPOINT")?;
    let access_key_id = env("TEST_S3_ACCESS_KEY_ID")?;
    // A unique set of buckets per run keeps parallel runs apart.
    let suffix = &uuid::Uuid::now_v7().simple().to_string()[20..];
    Some(S3Settings {
        endpoint,
        region: "us-east-1".to_owned(),
        access_key_id,
        secret_access_key: Secret::new(secret),
        buckets: S3Buckets {
            quarantine: format!("it-quarantine-{suffix}"),
            clean: format!("it-clean-{suffix}"),
            legal_hold: format!("it-legal-hold-{suffix}"),
        },
        allow_http: true,
        max_object_bytes: DEFAULT_MAX_OBJECT_BYTES,
    })
}

async fn store() -> Option<(S3ObjectStore, S3Settings)> {
    let secret = env("TEST_S3_SECRET_ACCESS_KEY")?;
    let settings = settings(secret)?;
    let store = S3ObjectStore::new(settings.clone(), Arc::new(SystemClock)).unwrap();
    for bucket in [Bucket::Quarantine, Bucket::Clean, Bucket::LegalHold] {
        store.create_bucket(bucket).await.unwrap();
        // Idempotent.
        store.create_bucket(bucket).await.unwrap();
    }
    Some((store, settings))
}

#[tokio::test]
async fn put_get_delete_round_trip() {
    let Some((store, _)) = store().await else {
        return;
    };
    let key = ObjectKey::random().unwrap();
    assert_eq!(store.get(Bucket::Quarantine, &key).await.unwrap(), None);

    let body = Bytes::from((0..=255u8).cycle().take(300_000).collect::<Vec<_>>());
    store
        .put(Bucket::Quarantine, &key, body.clone())
        .await
        .unwrap();
    assert_eq!(
        store.get(Bucket::Quarantine, &key).await.unwrap(),
        Some(body.clone())
    );
    // Buckets are separate.
    assert_eq!(store.get(Bucket::Clean, &key).await.unwrap(), None);

    // Overwrite, then delete twice (idempotent).
    store
        .put(Bucket::Quarantine, &key, Bytes::from_static(b"v2"))
        .await
        .unwrap();
    assert_eq!(
        store
            .get(Bucket::Quarantine, &key)
            .await
            .unwrap()
            .as_deref(),
        Some(&b"v2"[..])
    );
    store.delete(Bucket::Quarantine, &key).await.unwrap();
    store.delete(Bucket::Quarantine, &key).await.unwrap();
    assert_eq!(store.get(Bucket::Quarantine, &key).await.unwrap(), None);

    // Empty objects work too.
    store
        .put(Bucket::LegalHold, &key, Bytes::new())
        .await
        .unwrap();
    assert_eq!(
        store.get(Bucket::LegalHold, &key).await.unwrap(),
        Some(Bytes::new())
    );
}

#[tokio::test]
async fn a_wrong_secret_is_refused_without_leaking_it() {
    let Some((_, settings)) = store().await else {
        return;
    };
    let bad = S3ObjectStore::new(
        S3Settings {
            secret_access_key: Secret::new("not-the-secret-value"),
            ..settings
        },
        Arc::new(SystemClock),
    )
    .unwrap();
    let key = ObjectKey::random().unwrap();
    let err = bad
        .put(Bucket::Quarantine, &key, Bytes::from_static(b"x"))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("403"), "{err}");
    assert!(!err.contains("not-the-secret-value"), "{err}");
    assert!(!err.contains(key.as_str()), "{err}");
    let err = bad.get(Bucket::Quarantine, &key).await.unwrap_err();
    assert!(err.to_string().contains("403"), "{err}");
}

#[tokio::test]
async fn oversized_downloads_are_an_error() {
    let Some((store, settings)) = store().await else {
        return;
    };
    let key = ObjectKey::random().unwrap();
    store
        .put(Bucket::Clean, &key, Bytes::from(vec![7u8; 2048]))
        .await
        .unwrap();
    let small = S3ObjectStore::new(
        S3Settings {
            max_object_bytes: 1024,
            ..settings
        },
        Arc::new(SystemClock),
    )
    .unwrap();
    assert!(small.get(Bucket::Clean, &key).await.is_err());
}
