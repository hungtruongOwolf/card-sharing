-- identity module: "Findable by handle" (US-5a, SR-19). Owner: crates/identity (profile.rs).
-- Adults may turn it off; accounts under discovery.min_age can never be found whatever the flag
-- says (handle search checks the age on every query) and cannot turn it on (403 POLICY_DISALLOWED).

ALTER TABLE identity.profiles
    ADD COLUMN findable_by_handle BOOLEAN NOT NULL DEFAULT true;
