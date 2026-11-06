-- T-88: the activity tab (US-31 AC-31.3). One item per alert-worthy event and recipient, kept
-- whether or not a push was sent; ids only. Read-time filters re-check visibility (card still
-- visible, request still open, still friends). Retention 90 days.

CREATE TABLE notifications.activity_items (
    id          UUID PRIMARY KEY,
    account_id  UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    event_id    BIGINT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN (
                    'card.new', 'reaction.new', 'friend.request', 'friend.accepted', 'card.held')),
    subject_id  UUID NOT NULL,
    actor_id    UUID NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    UNIQUE (event_id, account_id, kind)
);
CREATE INDEX activity_items_account ON notifications.activity_items (account_id, id DESC);
CREATE INDEX activity_items_created ON notifications.activity_items (created_at);

-- "Read up to" watermark per account (item ids are UUIDv7, so time-ordered).
CREATE TABLE notifications.activity_reads (
    account_id  UUID PRIMARY KEY REFERENCES identity.accounts (id) ON DELETE CASCADE,
    read_up_to  UUID NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);
