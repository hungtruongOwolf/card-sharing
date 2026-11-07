-- identity module: invite-gated sign-up (sprint 10 phase 2, ADR-0012).
-- Owner: crates/identity (invite_codes.rs). Spec: context/wiki/architecture/data-model.md.
-- Codes are minted by staff on the command line (`api invites create`) and shown once; only the
-- SHA-256 of the canonical code (12 Crockford base32 characters) is stored. A use is a conditional
-- increment (`uses < max_uses`, not revoked, not expired) in the transaction that inserts the account.
-- `label` is a staff note (who or what the code is for), never shown to users.

CREATE TABLE identity.invite_codes (
    id          UUID PRIMARY KEY,
    code_hash   BYTEA NOT NULL UNIQUE CHECK (length(code_hash) = 32),
    label       TEXT NOT NULL CHECK (char_length(label) BETWEEN 1 AND 64),
    max_uses    INTEGER NOT NULL CHECK (max_uses BETWEEN 1 AND 10000),
    uses        INTEGER NOT NULL DEFAULT 0 CHECK (uses >= 0 AND uses <= max_uses),
    expires_at  TIMESTAMPTZ NOT NULL,
    revoked_at  TIMESTAMPTZ NULL,
    created_at  TIMESTAMPTZ NOT NULL
);

-- Wrong invite codes tried with one sign-in link; the attempt that reaches
-- `invites.max_attempts_per_link` (config/signup.yaml) expires the link.
ALTER TABLE identity.magic_links
    ADD COLUMN invite_attempts SMALLINT NOT NULL DEFAULT 0 CHECK (invite_attempts >= 0);

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
