//! AI effects module (Tier 2 cloud effects, genai-effects.md, ADR-0003): curated templates,
//! provider adapters behind [`EffectProvider`] (only [`FakeProvider`] today: no network, no keys),
//! explicit consent, a server-side credit ledger (reserve at submit, refund on refusal or
//! failure), input and output moderation hooks, and a C2PA manifest stub. Owns the `genai`
//! Postgres schema (migrations/*_genai*.sql). Who may use it (region, age) comes from the region
//! policy and is checked by the API.

pub mod c2pa;
pub mod job;
pub mod moderation;
pub mod personal_data;
pub mod provider;
pub mod service;
pub mod settings;

pub use job::RunEffectJob;
pub use moderation::{
    FAKE_INPUT_REFUSAL, FAKE_OUTPUT_REFUSAL, GateError, Gates, ImageGate, Verdict,
};
pub use personal_data::GenAiPersonalData;
pub use provider::{
    EffectProvider, EffectRequest, FakeProvider, ProviderError, UnconfiguredProvider,
};
pub use service::{
    EffectJob, GenAiError, GenAiService, JobStatus, NewEffectJob, RUN_JOB, RUN_MAX_ATTEMPTS,
    RunOutcome, Submitted,
};
pub use settings::{FakeTransform, GenAiSettings, SettingsError, Template};
