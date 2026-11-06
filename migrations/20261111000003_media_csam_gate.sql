-- T-72: known-CSAM hash gate (ADR-0005, content-safety.md, SR-26, US-28).
-- Every decoded image passes the `HashMatcher` port before it can leave quarantine. A match blocks
-- the media, preserves the raw upload in the legal-hold bucket and opens an incident; the
-- account is suspended through the outbox. Matcher outages keep the media `scanning` (retry with
-- backoff, never delivered unscanned).

ALTER TABLE media.media_objects
    ADD COLUMN scan_provider     TEXT NULL CHECK (length(scan_provider) <= 64),
    ADD COLUMN scan_list_version TEXT NULL CHECK (length(scan_list_version) <= 64),
    ADD COLUMN scanned_at        TIMESTAMPTZ NULL;

CREATE SCHEMA IF NOT EXISTS moderation;

-- Survives account deletion (no foreign keys): evidence and reporting duties outlive the account.
CREATE TABLE moderation.csam_incidents (
    id            UUID PRIMARY KEY,
    account_id    UUID NOT NULL,
    media_id      UUID NOT NULL UNIQUE,
    category      TEXT NOT NULL CHECK (category IN ('csam', 'harmful_abusive', 'test')),
    exact_match   BOOLEAN NOT NULL,
    provider      TEXT NOT NULL CHECK (length(provider) <= 64),
    list_version  TEXT NOT NULL CHECK (length(list_version) <= 64),
    provider_ref  TEXT NOT NULL CHECK (length(provider_ref) <= 256),
    evidence_key  TEXT NULL CHECK (evidence_key ~ '^[0-9a-f]{32}$'),
    matched_at    TIMESTAMPTZ NOT NULL,
    report_ref    TEXT NULL,
    reported_at   TIMESTAMPTZ NULL
);
-- Unreported incidents (the reporting process, CsamReporter port, is a follow-up).
CREATE INDEX csam_incidents_unreported ON moderation.csam_incidents (matched_at)
    WHERE reported_at IS NULL;
