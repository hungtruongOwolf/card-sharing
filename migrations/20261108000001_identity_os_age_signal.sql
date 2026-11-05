-- identity module: OS age signal (T-29, SR-25, legal L-3). Owner: crates/identity (age_service.rs).
-- The latest signal the app read from the OS (Apple Declared Age Range, Google Play Age Signals),
-- stored only while ingestion is enabled by config. A `minor` signal makes the account a minor even
-- when its DOB says adult; an `adult` signal never loosens what the DOB says.

ALTER TABLE identity.accounts
    ADD COLUMN os_age_signal        TEXT NULL CHECK (os_age_signal IN ('minor', 'adult')),
    ADD COLUMN os_age_signal_source TEXT NULL CHECK (os_age_signal_source IN (
                                        'apple_declared_age_range', 'google_play_age_signals')),
    ADD COLUMN os_age_signal_at     TIMESTAMPTZ NULL,
    ADD CONSTRAINT accounts_os_age_signal_complete CHECK (
        (os_age_signal IS NULL) = (os_age_signal_source IS NULL)
        AND (os_age_signal IS NULL) = (os_age_signal_at IS NULL));
