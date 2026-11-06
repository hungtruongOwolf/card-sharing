-- moderation module: moderation actions and two-person bans (T-146, US-27 AC-27.1, SR-32, SR-33).
-- Owner: crates/moderation (actions.rs). Every decision on a queue item is one append-only row
-- with the actor, the reason and the time. Bans of accounts older than 30 days need a second admin
-- (EM decision D6): the first request waits in `pending_bans` for 48 hours.

CREATE TABLE moderation.moderation_actions (
    id               UUID PRIMARY KEY,
    queue_item_id    UUID NULL REFERENCES moderation.queue_items (id),
    -- Staff account ids (no foreign key: the record outlives a staff account).
    actor_id         UUID NOT NULL,
    second_actor_id  UUID NULL,
    action           TEXT NOT NULL CHECK (action IN (
                         'dismiss', 'remove_content', 'revoke_link', 'warn', 'suspend', 'ban',
                         'release_hold', 'uphold_hold')),
    target_type      TEXT NOT NULL CHECK (target_type IN ('card', 'board', 'public_link', 'user')),
    target_id        UUID NOT NULL,
    reason           TEXT NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 500),
    -- End of a temporary suspension.
    until            TIMESTAMPTZ NULL,
    created_at       TIMESTAMPTZ NOT NULL,
    CHECK (second_actor_id IS NULL OR second_actor_id <> actor_id)
);
CREATE INDEX moderation_actions_target ON moderation.moderation_actions (target_type, target_id);
CREATE INDEX moderation_actions_item ON moderation.moderation_actions (queue_item_id);

CREATE TABLE moderation.pending_bans (
    id                 UUID PRIMARY KEY,
    queue_item_id      UUID NOT NULL REFERENCES moderation.queue_items (id),
    target_account_id  UUID NOT NULL,
    requested_by       UUID NOT NULL,
    reason             TEXT NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 500),
    created_at         TIMESTAMPTZ NOT NULL,
    expires_at         TIMESTAMPTZ NOT NULL,
    decided_at         TIMESTAMPTZ NULL,
    confirmed_by       UUID NULL,
    CHECK (confirmed_by IS NULL OR confirmed_by <> requested_by)
);
CREATE INDEX pending_bans_target ON moderation.pending_bans (target_account_id) WHERE decided_at IS NULL;
