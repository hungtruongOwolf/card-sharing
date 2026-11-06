-- moderation module: the review queue (T-143, US-27, SR-31). Owner: crates/moderation.
-- Every report joins the one open queue item of its target, so 50 reports about one card are one
-- item for a moderator, ordered by priority (worst reason) and the summed reputation weight of its
-- reporters. Nothing here acts on its own: items wait for a moderator.

CREATE TABLE moderation.queue_items (
    id                 UUID PRIMARY KEY,
    target_type        TEXT NOT NULL CHECK (target_type IN ('card', 'board', 'public_link', 'user')),
    target_id          UUID NOT NULL,
    target_owner_id    UUID NOT NULL,
    -- Highest priority among its reports (config/moderation.yaml).
    priority           SMALLINT NOT NULL CHECK (priority BETWEEN 0 AND 1000),
    -- Sum of the reporters' reputation weights (config/moderation.yaml `reputation`).
    weight             REAL NOT NULL CHECK (weight >= 0),
    report_count       INTEGER NOT NULL CHECK (report_count >= 1),
    status             TEXT NOT NULL CHECK (status IN ('open', 'in_review', 'actioned', 'dismissed')),
    first_reported_at  TIMESTAMPTZ NOT NULL,
    last_reported_at   TIMESTAMPTZ NOT NULL,
    decided_at         TIMESTAMPTZ NULL
);
-- One live item per target; a decided item stays as history and a new report opens a new one.
CREATE UNIQUE INDEX queue_items_live_target ON moderation.queue_items (target_type, target_id)
    WHERE status IN ('open', 'in_review');
CREATE INDEX queue_items_order ON moderation.queue_items (status, priority DESC, weight DESC, first_reported_at);
CREATE INDEX queue_items_target_owner ON moderation.queue_items (target_owner_id);

ALTER TABLE moderation.reports
    ADD COLUMN queue_item_id UUID NULL REFERENCES moderation.queue_items (id),
    ADD COLUMN weight REAL NOT NULL DEFAULT 1 CHECK (weight >= 0);
CREATE INDEX reports_queue_item ON moderation.reports (queue_item_id);
-- Reporter history for reputation (actioned vs dismissed reports of one reporter).
CREATE INDEX reports_reporter_status ON moderation.reports (reporter_id, status) WHERE reporter_id IS NOT NULL;
