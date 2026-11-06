//! Object storage port (`StorageProvider` in ADR-0005). Production uses S3-compatible storage
//! (Cloudflare R2, ADR-0008) behind this trait; local development and CI use the filesystem
//! adapter, tests the in-memory one. Keys are random and validated, so no adapter can be asked
//! for a path outside its bucket.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Mutex;

use bytes::Bytes;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Private buckets (ADR-0005). There is no public bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bucket {
    /// Raw uploads, never served to anyone.
    Quarantine,
    /// Re-encoded, scanned originals and derivatives; the only bucket behind the CDN.
    Clean,
    /// CSAM evidence (restricted access, object lock in production).
    LegalHold,
}

impl Bucket {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quarantine => "quarantine",
            Self::Clean => "clean",
            Self::LegalHold => "legal_hold",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "quarantine" => Some(Self::Quarantine),
            "clean" => Some(Self::Clean),
            "legal_hold" => Some(Self::LegalHold),
            _ => None,
        }
    }
}

/// An exact object key: 32 lowercase hex characters (128 random bits). No ids, names, slashes,
/// prefixes or wildcards can be expressed, which is what lets the URL signer refuse them.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ObjectKey(String);

impl ObjectKey {
    pub const LEN: usize = 32;

    /// A fresh random key.
    pub fn random() -> Result<Self, StoreError> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(Self(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// Parses a stored or requested key; anything but exactly 32 lowercase hex chars is refused.
    pub fn parse(value: &str) -> Option<Self> {
        (value.len() == Self::LEN
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ObjectKey {
    // Keys are capabilities once signed: keep them out of logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ObjectKey(..)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("object storage error: {0}")]
    Io(String),
}

/// Minimal object storage operations the pipeline needs.
pub trait ObjectStore: Send + Sync {
    fn put<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
        bytes: Bytes,
    ) -> BoxFuture<'a, Result<(), StoreError>>;
    /// `None` when the object does not exist.
    fn get<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
    ) -> BoxFuture<'a, Result<Option<Bytes>, StoreError>>;
    /// Idempotent: deleting a missing object succeeds.
    fn delete<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
    ) -> BoxFuture<'a, Result<(), StoreError>>;
}

/// Filesystem adapter: `<root>/<bucket>/<key>`. For local development and CI only (single
/// machine, no lifecycle rules, no object lock).
pub struct FsObjectStore {
    root: PathBuf,
}

impl FsObjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, bucket: Bucket, key: &ObjectKey) -> PathBuf {
        self.root.join(bucket.as_str()).join(key.as_str())
    }
}

fn io(err: std::io::Error) -> StoreError {
    StoreError::Io(err.kind().to_string())
}

impl ObjectStore for FsObjectStore {
    fn put<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
        bytes: Bytes,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        Box::pin(async move {
            let path = self.path(bucket, key);
            if let Some(dir) = path.parent() {
                tokio::fs::create_dir_all(dir).await.map_err(io)?;
            }
            // Write then rename, so a reader never sees a half-written object.
            let tmp = path.with_extension("partial");
            tokio::fs::write(&tmp, &bytes).await.map_err(io)?;
            tokio::fs::rename(&tmp, &path).await.map_err(io)
        })
    }

    fn get<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
    ) -> BoxFuture<'a, Result<Option<Bytes>, StoreError>> {
        Box::pin(async move {
            match tokio::fs::read(self.path(bucket, key)).await {
                Ok(bytes) => Ok(Some(Bytes::from(bytes))),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(err) => Err(io(err)),
            }
        })
    }

    fn delete<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        Box::pin(async move {
            match tokio::fs::remove_file(self.path(bucket, key)).await {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(io(err)),
            }
        })
    }
}

/// In-memory adapter for tests.
#[derive(Default)]
pub struct MemoryObjectStore {
    objects: Mutex<HashMap<(Bucket, String), Bytes>>,
}

impl MemoryObjectStore {
    fn objects(&self) -> std::sync::MutexGuard<'_, HashMap<(Bucket, String), Bytes>> {
        self.objects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Number of objects in `bucket`.
    pub fn count(&self, bucket: Bucket) -> usize {
        self.objects().keys().filter(|(b, _)| *b == bucket).count()
    }
}

impl ObjectStore for MemoryObjectStore {
    fn put<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
        bytes: Bytes,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        self.objects().insert((bucket, key.0.clone()), bytes);
        Box::pin(async { Ok(()) })
    }

    fn get<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
    ) -> BoxFuture<'a, Result<Option<Bytes>, StoreError>> {
        let found = self.objects().get(&(bucket, key.0.clone())).cloned();
        Box::pin(async move { Ok(found) })
    }

    fn delete<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        self.objects().remove(&(bucket, key.0.clone()));
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn keys_are_exact_random_hex() {
        let key = ObjectKey::random().unwrap();
        assert_eq!(key.as_str().len(), 32);
        assert_eq!(ObjectKey::parse(key.as_str()), Some(key.clone()));
        for bad in [
            "",
            "../etc/passwd",
            "*",
            "abc*",
            &key.as_str()[..31],
            &key.as_str().to_uppercase(),
        ] {
            assert_eq!(ObjectKey::parse(bad), None, "{bad:?}");
        }
        assert_eq!(format!("{key:?}"), "ObjectKey(..)");
    }

    #[tokio::test]
    async fn filesystem_round_trip() {
        let dir = std::env::temp_dir().join(format!("media-store-{}", uuid::Uuid::now_v7()));
        let store = FsObjectStore::new(&dir);
        let key = ObjectKey::random().unwrap();
        assert_eq!(store.get(Bucket::Quarantine, &key).await.unwrap(), None);
        store
            .put(Bucket::Quarantine, &key, Bytes::from_static(b"abc"))
            .await
            .unwrap();
        assert_eq!(
            store
                .get(Bucket::Quarantine, &key)
                .await
                .unwrap()
                .as_deref(),
            Some(&b"abc"[..])
        );
        // Buckets are separate.
        assert_eq!(store.get(Bucket::Clean, &key).await.unwrap(), None);
        store.delete(Bucket::Quarantine, &key).await.unwrap();
        store.delete(Bucket::Quarantine, &key).await.unwrap();
        assert_eq!(store.get(Bucket::Quarantine, &key).await.unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
