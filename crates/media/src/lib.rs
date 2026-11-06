//! Media module (ADR-0005): uploads stream into the private `quarantine` bucket, are sniffed by
//! magic bytes and header size before anything is stored, and wait for the `media.process` job.
//! Owns the `media` Postgres schema (migrations/*_media_*.sql). Nothing in this module decides
//! who may see a media object: callers check access, then ask for a signed URL.

pub mod personal_data;
pub mod pipeline;
pub mod process;
pub mod purge;
pub mod s3;
pub mod safety;
pub mod service;
pub mod signer;
pub mod sniff;
pub mod store;

pub use personal_data::MediaPersonalData;
pub use pipeline::{
    CSAM_MATCHED, FailureCode, MEDIA_SETTLED, ProcessMediaJob, ProcessSettings, sandboxed,
    sandboxed_decode,
};
pub use process::{Encoded, ProcessError, Processed, Variant};
pub use s3::{DEFAULT_MAX_OBJECT_BYTES, S3Buckets, S3ConfigError, S3ObjectStore, S3Settings};
pub use safety::{
    ClassifierVerdict, ContentClassifier, ContentGates, DEFAULT_NUDITY_THRESHOLD, FakeClassifier,
    FakeHashMatcher, HashMatcher, HashVerdict, MatchCategory, ScanInput, Unavailable,
    UnconfiguredClassifier, UnconfiguredHashMatcher,
};
pub use service::{
    GateState, MAX_UPLOAD_BYTES, MediaError, MediaKind, MediaRecord, MediaService, MediaState,
    PROCESS_JOB, Uploaded,
};
pub use signer::{SignedUrl, UrlAudience, UrlSigner, VerifyError};
pub use sniff::{ImageFormat, MAX_DIMENSION, SniffError, SniffOutcome, Sniffed, sniff};
pub use store::{Bucket, FsObjectStore, MemoryObjectStore, ObjectKey, ObjectStore, StoreError};
