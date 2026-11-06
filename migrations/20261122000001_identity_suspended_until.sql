-- identity module: temporary suspensions (T-147, US-27 AC-27.1 "suspend (temp)"). A moderator's
-- suspension carries an end; the worker's `identity.lift_suspensions` restores `active` after it.
-- NULL = indefinite (for example the automatic suspension after a known-CSAM match).
ALTER TABLE identity.accounts ADD COLUMN suspended_until TIMESTAMPTZ NULL;
CREATE INDEX accounts_suspension_due ON identity.accounts (suspended_until)
    WHERE status = 'suspended' AND suspended_until IS NOT NULL;
