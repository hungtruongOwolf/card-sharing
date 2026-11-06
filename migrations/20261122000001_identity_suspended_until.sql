-- identity module: temporary suspensions (T-147, US-27 AC-27.1 "suspend (temp)"). The column
-- `identity.accounts.suspended_until` exists since the first identity migration; it is now set by a
-- moderator's suspension, and the worker's `identity.lift_suspensions` restores `active` after it.
-- NULL = indefinite (for example the automatic suspension after a known-CSAM match).
CREATE INDEX accounts_suspension_due ON identity.accounts (suspended_until)
    WHERE status = 'suspended' AND suspended_until IS NOT NULL;
