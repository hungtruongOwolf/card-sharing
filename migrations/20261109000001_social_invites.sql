-- social module: personal invite links (US-4, SR-20). Owner: crates/social (invites.rs).
-- Spec: context/wiki/architecture/data-model.md. One live invite per account; a reset revokes it
-- (the old token then answers INVITE_EXPIRED) and inserts a new row. Lookups use the SHA-256 of
-- the token; the raw token is kept only encrypted (field cipher, bound to the owner) so the owner
-- can fetch the same link again.

CREATE SCHEMA IF NOT EXISTS social;

CREATE TABLE social.invites (
    id          UUID PRIMARY KEY,
    owner_id    UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    token_hash  BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    token_enc   BYTEA NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    revoked_at  TIMESTAMPTZ NULL
);
CREATE UNIQUE INDEX invites_one_live_per_owner ON social.invites (owner_id) WHERE revoked_at IS NULL;
CREATE INDEX invites_revoked ON social.invites (revoked_at) WHERE revoked_at IS NOT NULL;

-- Keep the new schema invisible to Supabase's Data API roles (ADR-0008); absent locally and in CI.
DO $$
DECLARE
    role_name TEXT;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['anon', 'authenticated'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
            EXECUTE format('REVOKE ALL ON SCHEMA social FROM %I', role_name);
            EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA social FROM %I', role_name);
        END IF;
    END LOOP;
END
$$;
