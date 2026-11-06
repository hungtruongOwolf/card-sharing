-- T-129: public board links (data-model.md `sharing`, US-24, SR-16).
-- The raw token (128-bit, base64url) is returned once at creation; only its SHA-256 is stored.
-- `board_id` has no foreign key: boards are soft-deleted and their links are revoked by the
-- sharing module (reason `board_deleted`); resolution also requires a live board.

CREATE SCHEMA IF NOT EXISTS sharing;

CREATE TABLE sharing.public_links (
    id              UUID PRIMARY KEY,
    board_id        UUID NOT NULL,
    owner_id        UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    token_hash      BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    created_at      TIMESTAMPTZ NOT NULL,
    expires_at      TIMESTAMPTZ NULL,
    revoked_at      TIMESTAMPTZ NULL,
    revoked_reason  TEXT NULL CHECK (revoked_reason IN (
                        'owner', 'moderator', 'anomaly', 'board_deleted', 'owner_ineligible')),
    view_count      BIGINT NOT NULL DEFAULT 0 CHECK (view_count >= 0),
    CHECK ((revoked_at IS NULL) = (revoked_reason IS NULL)),
    CHECK (expires_at IS NULL OR expires_at > created_at)
);
CREATE INDEX public_links_board_live ON sharing.public_links (board_id) WHERE revoked_at IS NULL;
CREATE INDEX public_links_owner ON sharing.public_links (owner_id);
-- Purge of links revoked more than 90 days ago.
CREATE INDEX public_links_revoked ON sharing.public_links (revoked_at) WHERE revoked_at IS NOT NULL;
