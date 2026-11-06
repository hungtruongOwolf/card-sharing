-- T-71: sanitized re-encodes and derivatives (ADR-0005, data-model.md `media`).
-- `media.process` decodes the upload in a sandboxed task, re-encodes it (lossless WebP, no
-- metadata) and stores the full-size image plus the recipient derivatives. The raw upload is kept
-- (`raw_storage_key`) until the content gates decide: it is the evidence copied to legal hold on a
-- CSAM match, and is deleted once the media is clean, held or failed for good.

ALTER TABLE media.media_objects
    ADD COLUMN processed_at  TIMESTAMPTZ NULL,
    ADD COLUMN raw_storage_key TEXT NULL UNIQUE CHECK (raw_storage_key ~ '^[0-9a-f]{32}$'),
    ADD COLUMN failure_code  TEXT NULL CHECK (failure_code IN (
                                 'missing_object', 'decode_error', 'decode_timeout',
                                 'decode_panic', 'encode_error')),
    ADD CONSTRAINT media_objects_failure_code CHECK ((state = 'failed') = (failure_code IS NOT NULL));

CREATE TABLE media.media_derivatives (
    id           UUID PRIMARY KEY,
    media_id     UUID NOT NULL REFERENCES media.media_objects (id) ON DELETE CASCADE,
    variant      TEXT NOT NULL CHECK (variant IN ('feed_1080', 'widget_512', 'thumb_256')),
    bucket       TEXT NOT NULL CHECK (bucket IN ('quarantine', 'clean', 'legal_hold')),
    storage_key  TEXT NOT NULL UNIQUE CHECK (storage_key ~ '^[0-9a-f]{32}$'),
    width        INT NOT NULL CHECK (width BETWEEN 1 AND 4096),
    height       INT NOT NULL CHECK (height BETWEEN 1 AND 4096),
    byte_size    INT NOT NULL CHECK (byte_size > 0),
    created_at   TIMESTAMPTZ NOT NULL,
    UNIQUE (media_id, variant)
);

