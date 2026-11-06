-- T-84: private reactions (US-17). One per recipient per card; only the card owner lists them.
-- `kind` is checked against the configured reaction ids by the API (config/client.yaml).

CREATE TABLE cards.reactions (
    card_id     UUID NOT NULL REFERENCES cards.cards (id) ON DELETE CASCADE,
    reactor_id  UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind ~ '^react\.[a-z0-9-]{1,32}$'),
    updated_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (card_id, reactor_id)
);
CREATE INDEX reactions_reactor ON cards.reactions (reactor_id);
