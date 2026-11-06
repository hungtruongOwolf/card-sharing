//! Staff accounts and their sessions (T-144, US-27, SR-32).
//!
//! A staff member is an ordinary account (signed in with Apple or Google) with a row in
//! `identity.staff_members`: a role (`moderator` or `admin`) and an encrypted TOTP secret. Staff
//! are added only from the command line (`api staff grant`), never through the API.
//!
//! Sign-in has two steps: a fresh provider proof of the account starts a 5-minute login challenge
//! (single use, 5 code attempts), and a TOTP code finishes it with a staff session: an opaque
//! cookie token plus a CSRF token, idle timeout 30 minutes, absolute 12 hours. Only SHA-256
//! hashes of the tokens are stored. Staff sessions are separate from app sessions: an app bearer
//! token never opens a staff route.

use platform::clock::SharedClock;
use platform::crypto::FieldCipher;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::token::{SecretToken, TokenHash};
use crate::totp;

/// Login challenge lifetime.
pub const LOGIN_TTL: Duration = Duration::minutes(5);
/// Code attempts per login challenge.
pub const MAX_CODE_ATTEMPTS: i16 = 5;
/// A staff session ends after this long without a request.
pub const STAFF_IDLE: Duration = Duration::minutes(30);
/// A staff session ends this long after the MFA step, whatever happens.
pub const STAFF_ABSOLUTE: Duration = Duration::hours(12);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaffRole {
    Moderator,
    Admin,
}

impl StaffRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Moderator => "moderator",
            Self::Admin => "admin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "moderator" => Some(Self::Moderator),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StaffError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("crypto error: {0}")]
    Crypto(String),
    /// Not a staff member, disabled, unknown/expired/used challenge, wrong code, unknown or ended
    /// session: callers answer all of them the same way.
    #[error("refused")]
    Refused,
}

/// A login challenge waiting for its TOTP code.
pub struct LoginChallenge {
    pub token: SecretToken,
    pub expires_at: OffsetDateTime,
}

/// A new staff session: the cookie token and the CSRF token (both shown once).
pub struct StaffSessionIssued {
    pub session_id: Uuid,
    pub token: SecretToken,
    pub csrf_token: SecretToken,
    pub role: StaffRole,
    pub idle_expires_at: OffsetDateTime,
    pub absolute_expires_at: OffsetDateTime,
}

/// The caller of a staff route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffPrincipal {
    pub account_id: Uuid,
    pub session_id: Uuid,
    pub role: StaffRole,
    pub csrf_hash: [u8; 32],
    pub idle_expires_at: OffsetDateTime,
    pub absolute_expires_at: OffsetDateTime,
}

/// One staff member as admins see them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffMember {
    pub account_id: Uuid,
    pub role: StaffRole,
    pub disabled: bool,
    pub created_at: OffsetDateTime,
}

pub struct StaffService {
    pool: PgPool,
    clock: SharedClock,
}

/// `login id, account, role, encrypted TOTP secret, last accepted step`.
type LoginRow = (Uuid, Uuid, String, Vec<u8>, Option<i64>);
/// `session id, account, role, CSRF hash, idle expiry, absolute expiry`.
type SessionRow = (Uuid, Uuid, String, Vec<u8>, OffsetDateTime, OffsetDateTime);

fn totp_aad(account: Uuid) -> Vec<u8> {
    format!("staff_totp:{account}").into_bytes()
}

fn new_token() -> Result<SecretToken, StaffError> {
    SecretToken::generate().map_err(|err| StaffError::Crypto(err.to_string()))
}

impl StaffService {
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self { pool, clock }
    }

    /// Makes `account` a staff member with `role` and a new TOTP secret (replacing any earlier
    /// one; re-enables a disabled member). Returns the secret for the authenticator app. CLI only.
    pub async fn grant(
        &self,
        account: Uuid,
        role: StaffRole,
        cipher: &FieldCipher,
    ) -> Result<Vec<u8>, StaffError> {
        let mut secret = vec![0u8; totp::SECRET_LEN];
        getrandom::fill(&mut secret).map_err(|err| StaffError::Crypto(err.to_string()))?;
        let encrypted = cipher
            .encrypt(&secret, &totp_aad(account))
            .map_err(|err| StaffError::Crypto(err.to_string()))?;
        let exists: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM identity.accounts WHERE id = $1")
                .bind(account)
                .fetch_optional(&self.pool)
                .await?;
        if exists.is_none() {
            return Err(StaffError::Refused);
        }
        sqlx::query(
            "INSERT INTO identity.staff_members
               (account_id, role, totp_secret_enc, totp_last_step, disabled_at, created_at)
             VALUES ($1, $2, $3, NULL, NULL, $4)
             ON CONFLICT (account_id) DO UPDATE
             SET role = EXCLUDED.role, totp_secret_enc = EXCLUDED.totp_secret_enc,
                 totp_last_step = NULL, disabled_at = NULL",
        )
        .bind(account)
        .bind(role.as_str())
        .bind(encrypted)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        Ok(secret)
    }

    /// Disables a staff member and ends their sessions. CLI only.
    pub async fn disable(&self, account: Uuid) -> Result<bool, StaffError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query(
            "UPDATE identity.staff_members SET disabled_at = $2
             WHERE account_id = $1 AND disabled_at IS NULL",
        )
        .bind(account)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query(
            "UPDATE identity.staff_sessions SET revoked_at = $2
             WHERE account_id = $1 AND revoked_at IS NULL",
        )
        .bind(account)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(changed > 0)
    }

    /// Starts a login for an account whose provider proof was just verified. Refused unless the
    /// account is an enabled staff member and the account itself is active.
    pub async fn start_login(&self, account: Uuid) -> Result<LoginChallenge, StaffError> {
        let enabled: Option<Uuid> = sqlx::query_scalar(
            "SELECT s.account_id FROM identity.staff_members s
             JOIN identity.accounts a ON a.id = s.account_id
             WHERE s.account_id = $1 AND s.disabled_at IS NULL AND a.status = 'active'",
        )
        .bind(account)
        .fetch_optional(&self.pool)
        .await?;
        if enabled.is_none() {
            return Err(StaffError::Refused);
        }
        let token = new_token()?;
        let now = self.clock.now();
        let expires_at = now + LOGIN_TTL;
        sqlx::query(
            "INSERT INTO identity.staff_logins (id, account_id, token_hash, attempts, expires_at, created_at)
             VALUES ($1, $2, $3, 0, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(account)
        .bind(token.hash().as_bytes())
        .bind(expires_at)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(LoginChallenge { token, expires_at })
    }

    /// Finishes a login with a TOTP code. A code is accepted once (replay of the same or an
    /// earlier step is refused); the challenge dies after success or [`MAX_CODE_ATTEMPTS`].
    pub async fn complete_login(
        &self,
        login_token: &str,
        code: &str,
        cipher: &FieldCipher,
    ) -> Result<StaffSessionIssued, StaffError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let row: Option<LoginRow> = sqlx::query_as(
            "UPDATE identity.staff_logins l SET attempts = l.attempts + 1
             FROM identity.staff_members s, identity.accounts a
             WHERE l.token_hash = $1 AND l.used_at IS NULL AND l.expires_at > $2
               AND l.attempts < $3
               AND s.account_id = l.account_id AND s.disabled_at IS NULL
               AND a.id = s.account_id AND a.status = 'active'
             RETURNING l.id, s.account_id, s.role, s.totp_secret_enc, s.totp_last_step",
        )
        .bind(TokenHash::of(login_token).as_bytes())
        .bind(now)
        .bind(MAX_CODE_ATTEMPTS)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((login_id, account, role, secret_enc, last_step)) = row else {
            tx.commit().await?;
            return Err(StaffError::Refused);
        };
        let secret = cipher
            .decrypt(&secret_enc, &totp_aad(account))
            .map_err(|err| StaffError::Crypto(err.to_string()))?;
        let step = totp::matching_step(&secret, code.trim(), totp::step_at(now.unix_timestamp()));
        let fresh = step.filter(|step| last_step.is_none_or(|last| *step > last));
        let Some(step) = fresh else {
            // The attempt counts; the challenge stays until it expires or runs out of attempts.
            tx.commit().await?;
            return Err(StaffError::Refused);
        };
        let role = StaffRole::parse(&role)
            .ok_or_else(|| StaffError::Crypto(format!("unknown staff role {role:?}")))?;
        sqlx::query("UPDATE identity.staff_logins SET used_at = $2 WHERE id = $1")
            .bind(login_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE identity.staff_members SET totp_last_step = $2 WHERE account_id = $1")
            .bind(account)
            .bind(step)
            .execute(&mut *tx)
            .await?;
        let token = new_token()?;
        let csrf_token = new_token()?;
        let session_id = Uuid::now_v7();
        let absolute_expires_at = now + STAFF_ABSOLUTE;
        let idle_expires_at = (now + STAFF_IDLE).min(absolute_expires_at);
        sqlx::query(
            "INSERT INTO identity.staff_sessions
               (id, account_id, token_hash, csrf_hash, mfa_at, idle_expires_at, absolute_expires_at,
                last_seen_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $5, $5)",
        )
        .bind(session_id)
        .bind(account)
        .bind(token.hash().as_bytes())
        .bind(csrf_token.hash().as_bytes())
        .bind(now)
        .bind(idle_expires_at)
        .bind(absolute_expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(StaffSessionIssued {
            session_id,
            token,
            csrf_token,
            role,
            idle_expires_at,
            absolute_expires_at,
        })
    }

    /// The staff caller behind a session cookie token; slides the idle timeout. Refused for an
    /// unknown, revoked, idle- or absolute-expired session, a disabled member or an account that
    /// is no longer active.
    pub async fn resolve(&self, session_token: &str) -> Result<StaffPrincipal, StaffError> {
        if session_token.is_empty() || session_token.len() > 256 {
            return Err(StaffError::Refused);
        }
        let now = self.clock.now();
        let row: Option<SessionRow> = sqlx::query_as(
            "UPDATE identity.staff_sessions ss
                 SET last_seen_at = $2,
                     idle_expires_at = LEAST($2 + make_interval(secs => $3), ss.absolute_expires_at)
                 FROM identity.staff_members s, identity.accounts a
                 WHERE ss.token_hash = $1 AND ss.revoked_at IS NULL
                   AND ss.idle_expires_at > $2 AND ss.absolute_expires_at > $2
                   AND s.account_id = ss.account_id AND s.disabled_at IS NULL
                   AND a.id = s.account_id AND a.status = 'active'
                 RETURNING ss.id, ss.account_id, s.role, ss.csrf_hash, ss.idle_expires_at,
                           ss.absolute_expires_at",
        )
        .bind(TokenHash::of(session_token).as_bytes())
        .bind(now)
        .bind(STAFF_IDLE.as_seconds_f64())
        .fetch_optional(&self.pool)
        .await?;
        let Some((session_id, account_id, role, csrf, idle, absolute)) = row else {
            return Err(StaffError::Refused);
        };
        let role = StaffRole::parse(&role).ok_or(StaffError::Refused)?;
        let csrf_hash: [u8; 32] = csrf.try_into().map_err(|_| StaffError::Refused)?;
        Ok(StaffPrincipal {
            account_id,
            session_id,
            role,
            csrf_hash,
            idle_expires_at: idle,
            absolute_expires_at: absolute,
        })
    }

    /// Ends one staff session.
    pub async fn logout(&self, session_id: Uuid) -> Result<(), StaffError> {
        sqlx::query(
            "UPDATE identity.staff_sessions SET revoked_at = $2 WHERE id = $1 AND revoked_at IS NULL",
        )
        .bind(session_id)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every staff member, newest first (admins only, at the route).
    pub async fn members(&self) -> Result<Vec<StaffMember>, StaffError> {
        let rows: Vec<(Uuid, String, Option<OffsetDateTime>, OffsetDateTime)> = sqlx::query_as(
            "SELECT account_id, role, disabled_at, created_at FROM identity.staff_members
             ORDER BY created_at DESC LIMIT 500",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(account_id, role, disabled_at, created_at)| {
                Some(StaffMember {
                    account_id,
                    role: StaffRole::parse(&role)?,
                    disabled: disabled_at.is_some(),
                    created_at,
                })
            })
            .collect())
    }
}

/// Whether `candidate` is the CSRF token whose hash is `expected` (constant time).
pub fn csrf_matches(expected: &[u8; 32], candidate: &str) -> bool {
    let actual = TokenHash::of(candidate);
    let diff = expected
        .iter()
        .zip(actual.as_bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    std::hint::black_box(diff) == 0
}
