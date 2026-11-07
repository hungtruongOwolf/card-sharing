-- Sprint 9: Tier 2 cloud AI effects on a fake provider (genai-effects.md, ADR-0003).
-- Consent per account, effect jobs, and an append-only credit ledger (reserve at submit, refund on
-- refusal or failure). Credits exist only here: a client can never grant itself any.

CREATE SCHEMA IF NOT EXISTS genai;

-- Explicit consent before a photo goes to a third-party AI (Apple 5.1.2(i)). One row per account;
-- `version` = the consent text the account agreed to (config/ai_effects.yaml consent_version).
CREATE TABLE genai.consents (
    account_id  UUID PRIMARY KEY REFERENCES identity.accounts (id) ON DELETE CASCADE,
    version     INTEGER NOT NULL CHECK (version >= 1),
    granted_at  TIMESTAMPTZ NOT NULL
);

CREATE TABLE genai.effect_jobs (
    id                UUID PRIMARY KEY,
    account_id        UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    -- Client-generated id: replaying a submit returns the same job (with Idempotency-Key).
    client_job_id     UUID NOT NULL,
    template_id       TEXT NOT NULL CHECK (char_length(template_id) BETWEEN 4 AND 64),
    template_version  INTEGER NOT NULL CHECK (template_version >= 1),
    input_media_id    UUID NOT NULL,
    status            TEXT NOT NULL CHECK (status IN ('queued', 'succeeded', 'refused', 'failed')),
    -- Why a job did not succeed (never shown in detail to clients beyond these codes).
    outcome           TEXT NULL CHECK (outcome IN (
                          'input_moderation', 'output_moderation', 'provider_refused',
                          'provider_failed', 'input_unavailable')),
    cost              INTEGER NOT NULL CHECK (cost >= 1),
    provider          TEXT NOT NULL,
    -- The generated image, uploaded as a card render through the media pipeline's gates.
    output_media_id   UUID NULL,
    -- C2PA manifest (stub until c2pa-rs signing; genai-effects "Safety & compliance").
    c2pa_manifest     JSONB NULL,
    created_at        TIMESTAMPTZ NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL,
    UNIQUE (account_id, client_job_id),
    CHECK ((status = 'succeeded') = (output_media_id IS NOT NULL)),
    CHECK ((status IN ('refused', 'failed')) = (outcome IS NOT NULL))
);
CREATE INDEX effect_jobs_account ON genai.effect_jobs (account_id, created_at DESC);
-- Cards look up whether their render is an AI output (the visible "AI" badge).
CREATE INDEX effect_jobs_output ON genai.effect_jobs (output_media_id) WHERE output_media_id IS NOT NULL;

-- Append-only: the balance is the sum of `delta`. Rows are never updated.
CREATE TABLE genai.credit_ledger (
    id          UUID PRIMARY KEY,
    account_id  UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    delta       INTEGER NOT NULL CHECK (delta <> 0),
    reason      TEXT NOT NULL CHECK (reason IN ('starter_grant', 'reserve', 'refund')),
    job_id      UUID NULL REFERENCES genai.effect_jobs (id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL,
    CHECK ((reason = 'starter_grant') = (job_id IS NULL)),
    CHECK ((reason = 'reserve') = (delta < 0))
);
CREATE INDEX credit_ledger_account ON genai.credit_ledger (account_id);
-- One starter grant per account; at most one reserve and one refund per job (replay-safe).
CREATE UNIQUE INDEX credit_ledger_one_starter ON genai.credit_ledger (account_id)
    WHERE reason = 'starter_grant';
CREATE UNIQUE INDEX credit_ledger_one_per_job ON genai.credit_ledger (job_id, reason)
    WHERE job_id IS NOT NULL;
