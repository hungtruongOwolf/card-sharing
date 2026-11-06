-- T-154: personal data exports (data-model.md `privacy`, US-30, SR-36). An export is built only
-- after a delay (all devices are told and it can be cancelled), then downloadable once, through a
-- single-use token (only its SHA-256 is stored), until `expires_at`.

CREATE TABLE privacy.data_exports (
    id                   UUID PRIMARY KEY,
    account_id           UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    status               TEXT NOT NULL CHECK (status IN (
                             'queued', 'building', 'ready', 'downloaded', 'expired', 'cancelled',
                             'failed')),
    requested_at         TIMESTAMPTZ NOT NULL,
    -- Not built before this (requested_at + delay).
    available_at         TIMESTAMPTZ NOT NULL,
    -- The ZIP in the clean bucket while ready or downloaded.
    storage_key          TEXT NULL CHECK (storage_key IS NULL OR storage_key ~ '^[0-9a-f]{32}$'),
    byte_size            BIGINT NULL CHECK (byte_size IS NULL OR byte_size >= 0),
    ready_at             TIMESTAMPTZ NULL,
    expires_at           TIMESTAMPTZ NULL,
    download_token_hash  BYTEA NULL CHECK (download_token_hash IS NULL
                             OR octet_length(download_token_hash) = 32),
    downloaded_at        TIMESTAMPTZ NULL,
    cancelled_at         TIMESTAMPTZ NULL,
    updated_at           TIMESTAMPTZ NOT NULL
);
CREATE INDEX data_exports_account ON privacy.data_exports (account_id, requested_at DESC);
CREATE INDEX data_exports_due ON privacy.data_exports (available_at) WHERE status IN ('queued', 'building');
CREATE INDEX data_exports_expiry ON privacy.data_exports (expires_at)
    WHERE status IN ('ready', 'downloaded');
