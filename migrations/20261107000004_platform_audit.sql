-- platform module: append-only audit log for auth events (T-17, SR-33).
-- Owner: crates/platform (audit.rs). Spec: context/wiki/architecture/data-model.md.
-- Rows hold ids and codes only: never tokens, emails, dates of birth or raw IPs.
--
-- Least privilege (ADR-0008): the application connects with a login role that is a member of
-- `app_rw` (NOLOGIN group created here); migrations run as the schema owner. `app_rw` may read and
-- write the module schemas but may only INSERT and SELECT audit rows: UPDATE, DELETE and TRUNCATE
-- are not granted, so audit rows cannot be changed by the application. Retention purges (2 years)
-- run as the owner role.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'app_rw') THEN
        CREATE ROLE app_rw NOLOGIN;
    END IF;
END
$$;

CREATE TABLE platform.audit_events (
    id          UUID PRIMARY KEY,
    actor_id    UUID NULL,
    actor_kind  TEXT NOT NULL CHECK (actor_kind IN ('user', 'staff', 'system')),
    action      TEXT NOT NULL CHECK (action ~ '^[a-z_]+\.[a-z_]+$'),
    -- `<type>:<uuid>`, e.g. `session:0192...`.
    target      TEXT NULL CHECK (target IS NULL OR target ~ '^[a-z_]+:[0-9a-f-]{36}$'),
    -- Keyed hash of the client IP when one is recorded (never the raw address).
    ip_hash     BYTEA NULL CHECK (ip_hash IS NULL OR length(ip_hash) = 32),
    -- Ids and codes only (same rule as outbox payloads).
    details     JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(details) = 'object'),
    at          TIMESTAMPTZ NOT NULL,
    CHECK ((actor_kind = 'system') = (actor_id IS NULL))
);
CREATE INDEX audit_events_target ON platform.audit_events (target, at) WHERE target IS NOT NULL;
CREATE INDEX audit_events_actor ON platform.audit_events (actor_id, at) WHERE actor_id IS NOT NULL;

GRANT USAGE ON SCHEMA identity, platform TO app_rw;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA identity, platform TO app_rw;
-- Tables created later by the migration role are reachable too.
ALTER DEFAULT PRIVILEGES IN SCHEMA identity, platform
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app_rw;
-- Append-only for the application.
REVOKE UPDATE, DELETE, TRUNCATE ON platform.audit_events FROM app_rw;

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
