-- social module: friend requests, friendships and blocks (US-5, US-26, SR-22). Owner: crates/social.
-- Spec: context/wiki/architecture/data-model.md.
--
-- A request is "open" while pending, declined or dropped and not expired: declines and drops are
-- silent, so the sender keeps seeing such a request as pending until it expires (14 days), and it
-- keeps its slot under the sender's pending cap. Only `pending` requests reach the recipient.

CREATE TABLE social.friend_requests (
    id            UUID PRIMARY KEY,
    from_user     UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    to_user       UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    source        TEXT NOT NULL CHECK (source IN ('invite', 'search')),
    status        TEXT NOT NULL CHECK (status IN ('pending', 'accepted', 'declined', 'cancelled', 'expired', 'dropped')),
    created_at    TIMESTAMPTZ NOT NULL,
    expires_at    TIMESTAMPTZ NOT NULL,
    responded_at  TIMESTAMPTZ NULL,
    CHECK (from_user <> to_user),
    CHECK (expires_at > created_at)
);
-- One open request per direction (pair dedupe).
CREATE UNIQUE INDEX friend_requests_one_open_pair ON social.friend_requests (from_user, to_user)
    WHERE status IN ('pending', 'declined', 'dropped');
CREATE INDEX friend_requests_incoming ON social.friend_requests (to_user, status, created_at DESC);
CREATE INDEX friend_requests_outgoing ON social.friend_requests (from_user, created_at DESC);
CREATE INDEX friend_requests_open_expiry ON social.friend_requests (expires_at)
    WHERE status IN ('pending', 'declined', 'dropped');

-- Canonical order (user_a < user_b): one row per friendship.
CREATE TABLE social.friendships (
    user_a      UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    user_b      UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_a, user_b),
    CHECK (user_a < user_b)
);
CREATE INDEX friendships_user_b ON social.friendships (user_b);

CREATE TABLE social.blocks (
    blocker_id  UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    blocked_id  UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (blocker_id, blocked_id),
    CHECK (blocker_id <> blocked_id)
);
CREATE INDEX blocks_blocked ON social.blocks (blocked_id);

DO $$
DECLARE
    role_name TEXT;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['anon', 'authenticated'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
            EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA social FROM %I', role_name);
        END IF;
    END LOOP;
END
$$;
