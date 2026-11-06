-- T-76 (+ the card side of T-73): delivery after both gates, the per-recipient delivery log and
-- classifier holds.

-- Which derivative went to which viewer (EM decision D2, MVP traceability). Ids only, no image
-- data; kept with the card.
CREATE TABLE cards.card_deliveries (
    card_id        UUID NOT NULL REFERENCES cards.cards (id) ON DELETE CASCADE,
    recipient_id   UUID NOT NULL,
    media_id       UUID NOT NULL,
    derivative_id  UUID NOT NULL,
    variant        TEXT NOT NULL CHECK (variant IN ('feed_1080', 'widget_512', 'thumb_256')),
    delivered_at   TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (card_id, recipient_id, derivative_id)
);
CREATE INDEX card_deliveries_recipient ON cards.card_deliveries (recipient_id, delivered_at DESC);

CREATE SCHEMA IF NOT EXISTS moderation;

-- A card held by the nudity classifier (AC-28.3): never delivered unless a moderator releases
-- it; unappealed holds expire after 30 days (the card is then deleted).
CREATE TABLE moderation.content_holds (
    id                UUID PRIMARY KEY,
    card_id           UUID NOT NULL UNIQUE,
    owner_id          UUID NOT NULL,
    media_id          UUID NOT NULL,
    classifier_model  TEXT NOT NULL CHECK (length(classifier_model) <= 64),
    score             REAL NOT NULL CHECK (score BETWEEN 0 AND 1),
    status            TEXT NOT NULL CHECK (status IN ('held', 'appealed', 'released', 'upheld', 'expired')),
    appeal_note       TEXT NULL CHECK (char_length(appeal_note) <= 500),
    appealed_at       TIMESTAMPTZ NULL,
    decided_by        UUID NULL,
    decided_at        TIMESTAMPTZ NULL,
    expires_at        TIMESTAMPTZ NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL
);
CREATE INDEX content_holds_queue ON moderation.content_holds (status, created_at);
CREATE INDEX content_holds_owner ON moderation.content_holds (owner_id);
