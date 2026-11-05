-- identity module: PKCE-bound email magic links (T-22, US-1, SR-1, AC-1.2/1.3).
-- Owner: crates/identity (magic_link.rs). Spec: context/wiki/architecture/data-model.md, ADR-0004.
-- The raw 256-bit token is never stored: only its SHA-256. Until the delivery job has sent the email,
-- `delivery_enc` holds the address and the raw token AEAD-encrypted (key API_DATA_ENCRYPTION_KEY, link id as
-- associated data); the job clears it after sending. Links expire after 15 minutes, are single use
-- (`used_at`, set only by the in-app verify call, T-23) and rows are purged after 24 hours.

CREATE TABLE identity.magic_links (
    id             UUID PRIMARY KEY,
    -- SHA-256 of the normalized (trimmed, lowercased) email: rate limits and the account subject.
    email_hash     BYTEA NOT NULL CHECK (length(email_hash) = 32),
    token_hash     BYTEA NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    -- PKCE S256 challenge from the requesting device (the verifier must match at verify time).
    pkce_challenge BYTEA NOT NULL CHECK (length(pkce_challenge) = 32),
    delivery_enc   BYTEA NULL,
    sent_at        TIMESTAMPTZ NULL,
    expires_at     TIMESTAMPTZ NOT NULL,
    used_at        TIMESTAMPTZ NULL,
    created_at     TIMESTAMPTZ NOT NULL
);
CREATE INDEX magic_links_email ON identity.magic_links (email_hash, created_at);
CREATE INDEX magic_links_created ON identity.magic_links (created_at);

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
