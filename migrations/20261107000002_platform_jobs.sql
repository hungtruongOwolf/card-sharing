-- platform module: Postgres job queue (T-15, ADR-0006).
-- Owner: crates/platform (jobs.rs). Spec: context/wiki/architecture/data-model.md.
-- Claimed with FOR UPDATE SKIP LOCKED; a running job holds a lease (`locked_until`) and is put back
-- in the queue when the lease ends (crashed worker). `unique_key` deduplicates enqueues (outbox
-- deliveries use `event_id:subscriber`). Done rows are purged after 7 days, dead rows after 30.

CREATE TABLE platform.jobs (
    id            UUID PRIMARY KEY,
    queue         TEXT NOT NULL CHECK (queue IN ('critical', 'default', 'bulk')),
    kind          TEXT NOT NULL CHECK (kind ~ '^[a-z0-9_.:-]{1,100}$'),
    unique_key    TEXT NULL UNIQUE CHECK (unique_key IS NULL OR length(unique_key) BETWEEN 1 AND 200),
    -- Ids only, never personal data (same rule as outbox events).
    payload       JSONB NOT NULL DEFAULT '{}'::jsonb,
    status        TEXT NOT NULL CHECK (status IN ('queued', 'running', 'done', 'dead')),
    run_at        TIMESTAMPTZ NOT NULL,
    attempts      INT NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts  INT NOT NULL CHECK (max_attempts BETWEEN 1 AND 100),
    locked_by     TEXT NULL,
    locked_until  TIMESTAMPTZ NULL,
    last_error    TEXT NULL CHECK (last_error IS NULL OR length(last_error) <= 2000),
    created_at    TIMESTAMPTZ NOT NULL,
    finished_at   TIMESTAMPTZ NULL,
    CHECK ((status = 'running') = (locked_until IS NOT NULL))
);
-- Next jobs to run per lane.
CREATE INDEX jobs_ready ON platform.jobs (queue, run_at) WHERE status = 'queued';
-- Leases to recover.
CREATE INDEX jobs_running_lease ON platform.jobs (locked_until) WHERE status = 'running';
-- Purge and dead-letter inspection.
CREATE INDEX jobs_finished ON platform.jobs (status, finished_at) WHERE status IN ('done', 'dead');

-- Wake idle workers of the lane (NOTIFY is delivered on commit, so a rolled-back enqueue never
-- wakes anyone). Workers also poll, so a missed notification only adds latency.
CREATE FUNCTION platform.jobs_notify() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify('platform_jobs', NEW.queue);
    RETURN NULL;
END
$$;
CREATE TRIGGER jobs_notify AFTER INSERT ON platform.jobs
    FOR EACH ROW EXECUTE FUNCTION platform.jobs_notify();

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
