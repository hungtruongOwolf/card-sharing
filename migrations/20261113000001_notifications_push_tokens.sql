-- T-85: push tokens (data-model.md `notifications`, api-mvp.md "Devices"). One token per device and
-- kind; a token belongs to one account at a time (re-registering it elsewhere moves it). Each row
-- is bound to the session that registered it: senders only use rows whose session is still live,
-- so logout, suspension and deletion stop pushes at once, and the worker prunes dead rows.

CREATE SCHEMA IF NOT EXISTS notifications;

CREATE TABLE notifications.push_tokens (
    id              UUID PRIMARY KEY,
    account_id      UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    device_id       UUID NOT NULL,
    -- identity.sessions id (no cross-module FK; liveness is checked through identity's predicate).
    session_id      UUID NOT NULL,
    provider        TEXT NOT NULL CHECK (provider IN ('apns', 'fcm')),
    kind            TEXT NOT NULL CHECK (kind IN ('alert', 'widget')),
    -- FieldCipher(token), associated data = account id. The token itself is never stored in clear.
    token_enc       BYTEA NOT NULL,
    -- SHA-256(token): global uniqueness without decrypting.
    token_hash      BYTEA NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL,
    invalidated_at  TIMESTAMPTZ NULL,
    UNIQUE (account_id, device_id, kind)
);
CREATE INDEX push_tokens_account_live ON notifications.push_tokens (account_id)
    WHERE invalidated_at IS NULL;
CREATE INDEX push_tokens_session ON notifications.push_tokens (session_id);

DO $$
DECLARE
    role_name TEXT;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['anon', 'authenticated'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
            EXECUTE format('REVOKE ALL ON SCHEMA notifications FROM %I', role_name);
            EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA notifications FROM %I', role_name);
        END IF;
    END LOOP;
END
$$;
