-- identity module: device attestation keys and verdicts (T-35, SR-13, SR-23).
-- Owner: crates/identity (attestation.rs). Spec: context/wiki/architecture/data-model.md.
-- One row per attested key: `ios:<App Attest key id>` (public key stored at registration) or
-- `android:<account>:<device>` (Play Integrity has no app-held key). Verdict counters per key feed
-- rate limiting and abuse review; `sign_count` is the last App Attest assertion counter (replay guard).

CREATE TABLE identity.attestation_keys (
    key_id          TEXT PRIMARY KEY CHECK (key_id ~ '^(ios|android):[A-Za-z0-9+/=:_-]{1,120}$'),
    account_id      UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    device_id       UUID NULL,
    platform        TEXT NOT NULL CHECK (platform IN ('ios', 'android')),
    -- Uncompressed P-256 point of an App Attest key; NULL until a key attestation passed.
    public_key      BYTEA NULL CHECK (public_key IS NULL OR length(public_key) = 65),
    sign_count      BIGINT NOT NULL DEFAULT 0 CHECK (sign_count >= 0),
    pass_count      INT NOT NULL DEFAULT 0 CHECK (pass_count >= 0),
    fail_count      INT NOT NULL DEFAULT 0 CHECK (fail_count >= 0),
    last_verdict    TEXT NULL CHECK (last_verdict IN ('pass', 'fail')),
    -- Stable reason code of the last failure (e.g. `nonce`, `replay`, `device_integrity`).
    last_failure    TEXT NULL CHECK (last_failure IS NULL OR last_failure ~ '^[a-z_]{1,40}$'),
    last_verdict_at TIMESTAMPTZ NULL,
    created_at      TIMESTAMPTZ NOT NULL
);
CREATE INDEX attestation_keys_account ON identity.attestation_keys (account_id);

-- Keep the new table invisible to Supabase's Data API roles (ADR-0008); absent locally and in CI.
DO $$
DECLARE
    role_name TEXT;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['anon', 'authenticated'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
            EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA identity FROM %I', role_name);
        END IF;
    END LOOP;
END
$$;
