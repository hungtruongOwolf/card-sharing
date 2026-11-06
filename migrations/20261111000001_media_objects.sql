-- T-70: media uploads in quarantine (ADR-0005, data-model.md `media`).
-- Storage keys are random 128-bit hex; no ids or names reach object storage. Raw uploads stay in
-- the private quarantine bucket until `media.process` re-encodes and scans them.

CREATE SCHEMA IF NOT EXISTS media;

CREATE TABLE media.media_objects (
    id            UUID PRIMARY KEY,
    owner_id      UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    kind          TEXT NOT NULL CHECK (kind IN ('card_render', 'mask', 'original', 'avatar')),
    state         TEXT NOT NULL CHECK (state IN (
                      'uploaded', 'scanning', 'clean', 'held', 'blocked', 'failed', 'deleted')),
    bucket        TEXT NOT NULL CHECK (bucket IN ('quarantine', 'clean', 'legal_hold')),
    storage_key   TEXT NOT NULL UNIQUE CHECK (storage_key ~ '^[0-9a-f]{32}$'),
    content_type  TEXT NOT NULL CHECK (content_type IN ('image/png', 'image/jpeg', 'image/webp')),
    -- Uploads are capped at 10 MiB by the API; lossless re-encodes (T-71) may be larger.
    byte_size     INT NOT NULL CHECK (byte_size BETWEEN 1 AND 134217728),
    width         INT NOT NULL CHECK (width BETWEEN 1 AND 4096),
    height        INT NOT NULL CHECK (height BETWEEN 1 AND 4096),
    sha256        BYTEA NOT NULL CHECK (octet_length(sha256) = 32),
    created_at    TIMESTAMPTZ NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL,
    UNIQUE (owner_id, sha256, kind)
);

-- Scan backlog (alert when the oldest pending upload is older than 5 minutes).
CREATE INDEX media_objects_pending ON media.media_objects (created_at)
    WHERE state IN ('uploaded', 'scanning');
