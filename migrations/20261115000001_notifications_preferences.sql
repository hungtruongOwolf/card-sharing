-- T-87: per-account notification toggles and quiet hours (US-31 AC-31.1). No row = defaults
-- (every type on, quiet hours 22:00-08:00 once the app has told us its time zone).

CREATE TABLE notifications.preferences (
    account_id     UUID PRIMARY KEY REFERENCES identity.accounts (id) ON DELETE CASCADE,
    card_new       BOOLEAN NOT NULL,
    reaction       BOOLEAN NOT NULL,
    friend         BOOLEAN NOT NULL,
    card_review    BOOLEAN NOT NULL,
    quiet_enabled  BOOLEAN NOT NULL,
    quiet_start    TIME NOT NULL,
    quiet_end      TIME NOT NULL,
    -- IANA zone name, validated against pg_timezone_names by the API; NULL = not known yet.
    time_zone      TEXT NULL CHECK (time_zone IS NULL OR time_zone ~ '^[A-Za-z0-9_+/-]{1,64}$'),
    updated_at     TIMESTAMPTZ NOT NULL,
    CHECK (quiet_start <> quiet_end)
);
