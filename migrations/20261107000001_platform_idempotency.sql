-- platform module: Idempotency-Key store (T-14, SR-12).
-- Owner: crates/platform (idempotency.rs). Spec: context/wiki/architecture/data-model.md and api-mvp.md.
-- One row per (caller, route template, key). The request hash detects a reused key with a different
-- request; the stored response is replayed for 24 h; `locked_until` is the lease of the request that
-- is still running (a crashed request frees the key when the lease ends).

CREATE SCHEMA IF NOT EXISTS platform;
REVOKE ALL ON SCHEMA platform FROM PUBLIC;

CREATE TABLE platform.idempotency_keys (
    user_id               UUID NOT NULL,
    route                 TEXT NOT NULL CHECK (length(route) BETWEEN 1 AND 200),
    key                   UUID NOT NULL,
    -- SHA-256 of method, path, query and body.
    request_hash          BYTEA NOT NULL CHECK (length(request_hash) = 32),
    -- NULL while the first request is still running.
    status_code           SMALLINT NULL CHECK (status_code IS NULL OR status_code BETWEEN 200 AND 499),
    response_content_type TEXT NULL CHECK (response_content_type IS NULL OR length(response_content_type) <= 100),
    response_body         BYTEA NULL CHECK (response_body IS NULL OR length(response_body) <= 65536),
    -- Fencing token of the current claim: only the request holding it may store the response.
    lease_id              UUID NOT NULL,
    locked_until          TIMESTAMPTZ NOT NULL,
    created_at            TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, route, key),
    CHECK ((status_code IS NULL) = (response_body IS NULL))
);
-- Purge of rows older than 24 h.
CREATE INDEX idempotency_keys_created ON platform.idempotency_keys (created_at);

-- Keep the schema invisible to Supabase's Data API roles (ADR-0008); absent locally and in CI.
DO $$
DECLARE
    role_name TEXT;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['anon', 'authenticated'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
            EXECUTE format('REVOKE ALL ON SCHEMA platform FROM %I', role_name);
            EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA platform FROM %I', role_name);
        END IF;
    END LOOP;
END
$$;
