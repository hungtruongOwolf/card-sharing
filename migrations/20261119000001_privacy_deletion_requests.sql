-- T-151: account deletion requests with a grace period (data-model.md `privacy`, US-29).
-- `account_id` has no foreign key: the row is the tombstone that outlives the account (erasure
-- is replayed from it after a backup restore, T-152).

CREATE SCHEMA IF NOT EXISTS privacy;

CREATE TABLE privacy.deletion_requests (
    account_id    UUID PRIMARY KEY,
    status        TEXT NOT NULL CHECK (status IN ('pending', 'cancelled', 'erasing', 'done')),
    requested_at  TIMESTAMPTZ NOT NULL,
    grace_until   TIMESTAMPTZ NOT NULL,
    cancelled_at  TIMESTAMPTZ NULL,
    erased_at     TIMESTAMPTZ NULL,
    updated_at    TIMESTAMPTZ NOT NULL,
    CHECK (grace_until > requested_at)
);
-- The erase job picks pending requests whose grace period ended.
CREATE INDEX deletion_requests_due ON privacy.deletion_requests (grace_until)
    WHERE status IN ('pending', 'erasing');
