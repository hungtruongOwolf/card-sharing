-- T-99: pins and the append-only op log (data-model.md `boards`, ADR-0007).
-- Pin ids are client-generated (UUIDv7) so pins can be added offline. `version` is per pin (the
-- `base_version` of updates); `changed_at_board_version` is the board version of the pin's last
-- change (delta sync cursor); `deleted_at` keeps a tombstone so delta reads can report removals.

CREATE TABLE boards.pins (
    id                        UUID PRIMARY KEY,
    board_id                  UUID NOT NULL REFERENCES boards.boards (id) ON DELETE CASCADE,
    created_by                UUID NOT NULL,
    kind                      TEXT NOT NULL CHECK (kind IN ('card', 'decoration')),
    -- Logical reference into the cards module (no FK across modules).
    card_id                   UUID NULL,
    -- Registry reference `{id, version, params}` (validated by the API); no user content.
    decoration                JSONB NULL,
    x                         DOUBLE PRECISION NOT NULL CHECK (x BETWEEN 0 AND 1),
    y                         DOUBLE PRECISION NOT NULL CHECK (y BETWEEN 0 AND 1),
    scale                     DOUBLE PRECISION NOT NULL CHECK (scale BETWEEN 0.1 AND 10),
    rotation                  DOUBLE PRECISION NOT NULL CHECK (rotation >= -180 AND rotation < 180),
    z_key                     TEXT NOT NULL CHECK (z_key ~ '^[0-9A-Za-z]{1,32}$'),
    version                   BIGINT NOT NULL CHECK (version >= 1),
    changed_at_board_version  BIGINT NOT NULL,
    updated_by                UUID NOT NULL,
    created_at                TIMESTAMPTZ NOT NULL,
    updated_at                TIMESTAMPTZ NOT NULL,
    deleted_at                TIMESTAMPTZ NULL,
    CHECK ((kind = 'card') = (card_id IS NOT NULL)),
    CHECK ((kind = 'decoration') = (decoration IS NOT NULL))
);
CREATE INDEX pins_board_changes ON boards.pins (board_id, changed_at_board_version);
CREATE INDEX pins_board_live ON boards.pins (board_id) WHERE deleted_at IS NULL;
-- Card-delete fan-out and "pins of this card" lookups.
CREATE INDEX pins_card ON boards.pins (card_id) WHERE card_id IS NOT NULL;

-- Who changed what, in board-version order. Only applied ops are logged; `op_id` is the client's
-- idempotency key (a replay is answered from here and changes nothing).
CREATE TABLE boards.board_ops (
    board_id    UUID NOT NULL REFERENCES boards.boards (id) ON DELETE CASCADE,
    seq         BIGINT NOT NULL,
    op_id       UUID NOT NULL UNIQUE,
    actor_id    UUID NOT NULL,
    pin_id      UUID NOT NULL,
    op          JSONB NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (board_id, seq)
);
