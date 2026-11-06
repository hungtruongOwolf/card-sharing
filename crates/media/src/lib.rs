//! Media module (ADR-0005): uploads stream into the private `quarantine` bucket, are sniffed by
//! magic bytes and header size before anything is stored, and wait for the `media.process` job.
//! Owns the `media` Postgres schema (migrations/*_media_*.sql). Nothing in this module decides
//! who may see a media object: callers check access, then ask for a signed URL.

pub mod pipeline;
pub mod process;
pub mod service;
pub mod sniff;
pub mod store;

pub use pipeline::{FailureCode, ProcessMediaJob, ProcessSettings, sandboxed};
pub use process::{Encoded, ProcessError, Processed, Variant};
pub use service::{
    MAX_UPLOAD_BYTES, MediaError, MediaKind, MediaRecord, MediaService, MediaState, PROCESS_JOB,
    Uploaded,
};
pub use sniff::{ImageFormat, MAX_DIMENSION, SniffError, SniffOutcome, Sniffed, sniff};
pub use store::{Bucket, FsObjectStore, MemoryObjectStore, ObjectKey, ObjectStore, StoreError};
