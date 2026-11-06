-- T-86: one row per alert push decision (sent, batched, capped, no target, ...). It de-duplicates
-- re-run jobs (one decision per outbox event, recipient and kind), enforces the daily cap
-- (AC-31.4) and batches new-card pushes (US-31). Ids and codes only; pruned after 7 days.

CREATE TABLE notifications.push_log (
    event_id    BIGINT NOT NULL,
    account_id  UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind ~ '^[a-z]+\.[a-z_]+$'),
    outcome     TEXT NOT NULL CHECK (outcome IN (
                    'pending', 'sent', 'sent_batch', 'batched', 'capped', 'no_target',
                    'not_configured', 'failed')),
    created_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (event_id, account_id, kind)
);
CREATE INDEX push_log_recent ON notifications.push_log (account_id, created_at DESC);
