-- identity module: the account's own profile (US-3). Owner: crates/identity (profile.rs).
-- Spec: context/wiki/architecture/data-model.md. Display names are sanitized and length-checked in
-- grapheme clusters by the app (platform::text); the CHECK below is only a storage bound
-- (30 clusters x 16 code points).

CREATE TABLE identity.profiles (
    account_id   UUID PRIMARY KEY REFERENCES identity.accounts (id) ON DELETE CASCADE,
    display_name TEXT NULL CHECK (display_name IS NULL OR char_length(display_name) BETWEEN 1 AND 480),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

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
