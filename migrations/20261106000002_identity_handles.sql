-- identity module: unique handles with change cooldown and reservation (US-3 AC-3.3, SR-19).
-- Owner: crates/identity (profile.rs). Spec: context/wiki/architecture/data-model.md.
-- Handles are stored lowercase (the app lowercases input), so the plain UNIQUE constraint is
-- case-insensitive uniqueness; it is also the race guard of last resort (HANDLE_TAKEN).

ALTER TABLE identity.profiles
    ADD COLUMN handle             TEXT NULL UNIQUE CHECK (handle IS NULL OR handle ~ '^[a-z0-9._]{3,20}$'),
    -- First claim (starts the free-change grace window); never reset.
    ADD COLUMN handle_claimed_at  TIMESTAMPTZ NULL,
    -- Last claim or change (the change cooldown counts from here).
    ADD COLUMN handle_changed_at  TIMESTAMPTZ NULL,
    ADD CONSTRAINT profiles_handle_times CHECK (
        (handle IS NULL AND handle_claimed_at IS NULL AND handle_changed_at IS NULL)
        OR (handle IS NOT NULL AND handle_claimed_at IS NOT NULL AND handle_changed_at IS NOT NULL));

-- A released handle stays reserved for its previous owner until `reserved_until`, so nobody can
-- take it over to impersonate them. No FK: the reservation outlives a deleted account.
CREATE TABLE identity.handle_reservations (
    handle         TEXT PRIMARY KEY CHECK (handle ~ '^[a-z0-9._]{3,20}$'),
    released_by    UUID NOT NULL,
    reserved_until TIMESTAMPTZ NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX handle_reservations_expiry ON identity.handle_reservations (reserved_until);

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
