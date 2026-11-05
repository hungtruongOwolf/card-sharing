-- platform module: transactional outbox (T-16, ADR-0006).
-- Owner: crates/platform (outbox.rs). Spec: context/wiki/architecture/data-model.md.
-- Modules insert events in their business transaction (ids only, no personal data). The worker's
-- relay hands each event to its in-process subscribers, which enqueue jobs keyed
-- `event_id:subscriber`, and marks it published in the same transaction. Published rows are purged
-- after 7 days.

CREATE TABLE platform.outbox_events (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    topic        TEXT NOT NULL CHECK (topic ~ '^[a-z0-9_.]{1,100}$'),
    payload      JSONB NOT NULL CHECK (jsonb_typeof(payload) = 'object' AND pg_column_size(payload) <= 8192),
    trace_id     TEXT NULL CHECK (trace_id IS NULL OR length(trace_id) <= 64),
    created_at   TIMESTAMPTZ NOT NULL,
    published_at TIMESTAMPTZ NULL
);
-- Unpublished events in order, per topic (the relay only takes topics it has subscribers for).
CREATE INDEX outbox_events_pending ON platform.outbox_events (topic, id) WHERE published_at IS NULL;
-- Purge.
CREATE INDEX outbox_events_published ON platform.outbox_events (published_at) WHERE published_at IS NOT NULL;

-- Keep the new table invisible to Supabase's Data API roles (ADR-0008); absent locally and in CI.
DO $$
DECLARE
    role_name TEXT;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['anon', 'authenticated'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
            EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA platform FROM %I', role_name);
        END IF;
    END LOOP;
END
$$;
