-- T-130: public board snapshots and their watermarked images (data-model.md `sharing`, AC-24.3,
-- AC-24.5, AC-24.7, SR-18). One snapshot per board with at least one live link: the owner's own
-- cards (as watermarked derivatives) and neutral private tiles for every other card pin. No
-- friend's card id, media key or caption is ever stored here.

CREATE TABLE sharing.public_snapshots (
    board_id       UUID PRIMARY KEY,
    owner_id       UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    board_version  BIGINT NOT NULL,
    -- {title, backdrop, pins: [{kind: card|private_tile|decoration, ...}]}; card pins carry the
    -- owner's card id (re-checked when served, never sent) and the watermarked object key.
    snapshot       JSONB NOT NULL,
    -- SHA-256 of `snapshot`: an unchanged rebuild keeps the OG image and writes nothing.
    content_hash   BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    -- Watermarked 1200x630 OG image in the clean bucket (AC-24.7).
    og_key         TEXT NOT NULL CHECK (og_key ~ '^[0-9a-f]{32}$'),
    built_at       TIMESTAMPTZ NOT NULL
);
CREATE INDEX public_snapshots_owner ON sharing.public_snapshots (owner_id);

-- Watermarked derivative (<= 1080 px) of one of the owner's cards, shared by every snapshot that
-- shows the card. Only cards whose owner is the board owner ever get a row.
CREATE TABLE sharing.public_media (
    card_id     UUID PRIMARY KEY,
    owner_id    UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    object_key  TEXT NOT NULL UNIQUE CHECK (object_key ~ '^[0-9a-f]{32}$'),
    width       INTEGER NOT NULL CHECK (width BETWEEN 1 AND 1080),
    height      INTEGER NOT NULL CHECK (height BETWEEN 1 AND 1080),
    created_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX public_media_owner ON sharing.public_media (owner_id);
