-- identity module: "Findable by handle" is opt-in for anyone who signed up under the discovery age
-- (EM decision 2026-11-05). Owner: crates/identity (age_gate.rs, profile.rs).
-- The age gate now writes the starting value when it admits an account: on for sign-ups at or above
-- discovery.min_age, off for younger ones, who stay hidden after reaching that age until they turn
-- it on themselves. Rows written any other way start hidden.
--
-- Existing rows cannot be classified here (the date of birth is AEAD-encrypted), so every existing
-- row is reset to hidden: the safe side. No deployed environment holds real accounts yet.

ALTER TABLE identity.profiles
    ALTER COLUMN findable_by_handle SET DEFAULT false;

UPDATE identity.profiles SET findable_by_handle = false WHERE findable_by_handle;
