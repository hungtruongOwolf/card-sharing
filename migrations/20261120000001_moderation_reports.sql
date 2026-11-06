-- moderation module: reports (T-141, US-26 AC-26.1, SR-31). Owner: crates/moderation.
-- Spec: context/wiki/architecture/data-model.md (`reports`) and api-mvp.md "Safety, moderation".
-- One reporter has at most one report per target (dedupe indexes below). Reports never trigger an
-- automatic action. `reporter_id` is cleared (and the note dropped) when the reporter's account is
-- erased; the report itself stays for moderation.

CREATE SCHEMA IF NOT EXISTS moderation;

CREATE TABLE moderation.reports (
    id                UUID PRIMARY KEY,
    source            TEXT NOT NULL CHECK (source IN ('user', 'guest', 'appeal')),
    -- Set for signed-in reporters only (NULL after the reporter's account is erased).
    reporter_id       UUID NULL CHECK (reporter_id IS NULL OR source = 'user'),
    -- Keyed hash of a guest's client IP (T-142); never the IP itself.
    reporter_ip_hash  BYTEA NULL CHECK (
                          (source = 'guest') = (reporter_ip_hash IS NOT NULL)
                          AND (reporter_ip_hash IS NULL OR octet_length(reporter_ip_hash) = 32)),
    target_type       TEXT NOT NULL CHECK (target_type IN ('card', 'board', 'public_link', 'user')),
    target_id         UUID NOT NULL,
    -- The account whose content or account is reported (moderation lookups, enforcement).
    target_owner_id   UUID NOT NULL,
    reason            TEXT NOT NULL CHECK (reason IN (
                          'sexual', 'minor_safety', 'harassment', 'violence', 'hate', 'spam',
                          'other')),
    note              TEXT NULL CHECK (char_length(note) <= 500),
    -- From config/moderation.yaml at filing time (higher first).
    priority          SMALLINT NOT NULL CHECK (priority BETWEEN 0 AND 1000),
    status            TEXT NOT NULL CHECK (status IN ('open', 'in_review', 'actioned', 'dismissed')),
    created_at        TIMESTAMPTZ NOT NULL
);
CREATE UNIQUE INDEX reports_user_dedupe ON moderation.reports (reporter_id, target_type, target_id)
    WHERE source = 'user';
CREATE UNIQUE INDEX reports_guest_dedupe ON moderation.reports (reporter_ip_hash, target_type, target_id)
    WHERE source = 'guest';
CREATE INDEX reports_queue ON moderation.reports (status, priority DESC, created_at);
CREATE INDEX reports_target ON moderation.reports (target_type, target_id);
CREATE INDEX reports_target_owner ON moderation.reports (target_owner_id);
