-- T-75: cards and their recipients (data-model.md `cards`, api-mvp.md "Cards").
-- Recipients are fixed at send time (current friends only); `delivered_at` is set once both
-- content gates passed, so nobody can see a card before that.

CREATE SCHEMA IF NOT EXISTS cards;

CREATE TABLE cards.cards (
    id                 UUID PRIMARY KEY,
    owner_id           UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    client_card_id     UUID NOT NULL,
    status             TEXT NOT NULL CHECK (status IN (
                           'pending_scan', 'held_review', 'active', 'blocked', 'failed', 'deleted')),
    render_media_id    UUID NOT NULL,
    mask_media_id      UUID NULL,
    original_media_id  UUID NULL,
    style              JSONB NOT NULL,
    caption            TEXT NULL CHECK (char_length(caption) BETWEEN 1 AND 400),
    alt_text           TEXT NULL CHECK (char_length(alt_text) BETWEEN 1 AND 800),
    audience_mode      TEXT NOT NULL CHECK (audience_mode IN ('all', 'subset')),
    schema_version     SMALLINT NOT NULL DEFAULT 1,
    sent_at            TIMESTAMPTZ NOT NULL,
    delivered_at       TIMESTAMPTZ NULL,
    deleted_at         TIMESTAMPTZ NULL,
    updated_at         TIMESTAMPTZ NOT NULL,
    UNIQUE (owner_id, client_card_id)
);
CREATE INDEX cards_owner ON cards.cards (owner_id, sent_at DESC);
-- Cards waiting for their media's gates, found by media id when a scan settles.
CREATE INDEX cards_pending_render ON cards.cards (render_media_id) WHERE status = 'pending_scan';
CREATE INDEX cards_pending_mask ON cards.cards (mask_media_id) WHERE status = 'pending_scan';
CREATE INDEX cards_pending_original ON cards.cards (original_media_id) WHERE status = 'pending_scan';

CREATE TABLE cards.card_recipients (
    card_id       UUID NOT NULL REFERENCES cards.cards (id) ON DELETE CASCADE,
    recipient_id  UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    -- Denormalized for the feed filter.
    sender_id     UUID NOT NULL,
    delivered_at  TIMESTAMPTZ NULL,
    hidden_at     TIMESTAMPTZ NULL,
    PRIMARY KEY (card_id, recipient_id),
    CHECK (recipient_id <> sender_id)
);
CREATE INDEX card_recipients_feed ON cards.card_recipients (recipient_id, delivered_at DESC, card_id)
    WHERE delivered_at IS NOT NULL AND hidden_at IS NULL;
