-- identity module: staff accounts, login challenges and staff sessions (T-144, US-27, SR-32).
-- Owner: crates/identity (staff.rs). Spec: data-model.md `staff_members`, api-mvp.md "Staff".
-- Staff are ordinary accounts promoted from the command line (`api staff grant`). Second factor:
-- TOTP (WebAuthn later). Staff sessions are a separate table from app sessions, so no app token
-- can ever open a staff route. Only SHA-256 hashes of tokens are stored.

CREATE TABLE identity.staff_members (
    account_id       UUID PRIMARY KEY REFERENCES identity.accounts (id) ON DELETE CASCADE,
    role             TEXT NOT NULL CHECK (role IN ('moderator', 'admin')),
    -- FieldCipher(TOTP secret), associated data `staff_totp:<account id>`.
    totp_secret_enc  BYTEA NOT NULL,
    -- Last accepted TOTP step: a code is accepted once.
    totp_last_step   BIGINT NULL,
    disabled_at      TIMESTAMPTZ NULL,
    created_at       TIMESTAMPTZ NOT NULL
);

-- Step one of a staff sign-in (provider proof done, TOTP code pending): single use, 5 minutes.
CREATE TABLE identity.staff_logins (
    id          UUID PRIMARY KEY,
    account_id  UUID NOT NULL REFERENCES identity.staff_members (account_id) ON DELETE CASCADE,
    token_hash  BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    attempts    SMALLINT NOT NULL CHECK (attempts >= 0),
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ NULL,
    created_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX staff_logins_account ON identity.staff_logins (account_id);

-- A signed-in staff member (cookie `__Host-staff_session`): idle 30 min, absolute 12 h.
CREATE TABLE identity.staff_sessions (
    id                   UUID PRIMARY KEY,
    account_id           UUID NOT NULL REFERENCES identity.staff_members (account_id) ON DELETE CASCADE,
    token_hash           BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    csrf_hash            BYTEA NOT NULL CHECK (octet_length(csrf_hash) = 32),
    mfa_at               TIMESTAMPTZ NOT NULL,
    idle_expires_at      TIMESTAMPTZ NOT NULL,
    absolute_expires_at  TIMESTAMPTZ NOT NULL,
    last_seen_at         TIMESTAMPTZ NOT NULL,
    revoked_at           TIMESTAMPTZ NULL,
    created_at           TIMESTAMPTZ NOT NULL
);
CREATE INDEX staff_sessions_account ON identity.staff_sessions (account_id) WHERE revoked_at IS NULL;
