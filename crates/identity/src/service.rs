//! Accounts and opaque rotating sessions (ADR-0004, AC-1.4, SR-2).
//!
//! - Access token: 256-bit opaque, 10 min, stored as SHA-256; lookups cached in Redis for at most
//!   the token's remaining lifetime. Revocation deletes the cache entry, so it is immediate.
//! - Refresh token: 256-bit opaque, single use. `refresh` consumes it and returns a new pair.
//!   Presenting a consumed token revokes the whole session (family) — except a retry of the
//!   immediately previous token within the grace window while its successor is still unused.
//! - Family lifetime: idle 60 days, absolute 180 days.
//!
//! All timestamps come from the injected clock (never the database's `now()`), so expiry and
//! grace windows are testable.

use platform::audit::{self, Actor, AuditAction, AuditEntry};
use platform::cache::ConnectionManager;
use platform::clock::SharedClock;
use platform::ids::new_id;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::cache::SessionCache;
use crate::model::{AccountStatus, RegionSignals, SessionScope};
use crate::token::{RandomnessError, SecretToken, TokenHash};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPolicy {
    pub access_ttl: Duration,
    pub idle_ttl: Duration,
    pub absolute_ttl: Duration,
    /// How long the immediately previous refresh token stays usable after rotation, so a
    /// client retry after a lost response is not mistaken for theft.
    pub refresh_grace: Duration,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            access_ttl: Duration::minutes(10),
            idle_ttl: Duration::days(60),
            absolute_ttl: Duration::days(180),
            refresh_grace: Duration::seconds(30),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Apple,
    Google,
    Email,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Apple => "apple",
            Self::Google => "google",
            Self::Email => "email",
        }
    }
}

/// A verified external identity (from a validated ID token or magic link).
#[derive(Debug, Clone)]
pub struct VerifiedIdentity {
    pub provider: Provider,
    /// Provider `sub` (or email hash). Accounts are keyed by provider + subject only: the same
    /// email under two providers is two accounts (AC-1.3, no auto-linking).
    pub subject: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Ios,
    Android,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: Uuid,
    pub platform: Platform,
    pub app_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeReason {
    Logout,
    LogoutAll,
    RefreshReuse,
    Suspended,
    Deleted,
    Staff,
}

impl RevokeReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Logout => "logout",
            Self::LogoutAll => "logout_all",
            Self::RefreshReuse => "refresh_reuse",
            Self::Suspended => "suspended",
            Self::Deleted => "deleted",
            Self::Staff => "staff",
        }
    }
}

/// A freshly issued token pair. Raw tokens are only ever returned here.
#[derive(Debug)]
pub struct IssuedSession {
    pub session_id: Uuid,
    pub account_id: Uuid,
    pub access_token: SecretToken,
    pub access_expires_at: OffsetDateTime,
    pub refresh_token: SecretToken,
}

/// The live session behind an access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPrincipal {
    pub account_id: Uuid,
    pub session_id: Uuid,
    pub scope: SessionScope,
    pub status: AccountStatus,
    pub account_created_at: OffsetDateTime,
    pub device_id: Option<Uuid>,
    pub access_expires_at: OffsetDateTime,
}

/// What the account itself may see about its own account (`GET /v1/me`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSummary {
    pub id: Uuid,
    pub status: AccountStatus,
    pub region_signals: RegionSignals,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Randomness(#[from] RandomnessError),
    #[error("corrupt row: {0}")]
    Corrupt(String),
    #[error("audit write failed: {0}")]
    Audit(#[from] platform::outbox::OutboxError),
}

#[derive(Debug, thiserror::Error)]
pub enum AccessError {
    #[error("unknown access token")]
    Unknown,
    #[error("access token expired")]
    Expired,
    #[error("session revoked or ended")]
    Revoked,
    /// The session was ended because the account is suspended or banned (T-147): the caller
    /// answers 403 `ACCOUNT_SUSPENDED` instead of a plain "signed out".
    #[error("account suspended or banned")]
    Blocked,
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error("unknown refresh token")]
    Unknown,
    /// Session revoked or past its idle/absolute lifetime.
    #[error("session revoked or ended")]
    Revoked,
    /// A consumed refresh token was replayed; the family has now been revoked.
    #[error("refresh token reuse detected")]
    Reused,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for RefreshError {
    fn from(err: sqlx::Error) -> Self {
        Self::Store(StoreError::Database(err))
    }
}

impl From<sqlx::Error> for AccessError {
    fn from(err: sqlx::Error) -> Self {
        Self::Store(StoreError::Database(err))
    }
}

pub struct IdentityService {
    pub(crate) pool: PgPool,
    pub(crate) cache: SessionCache,
    pub(crate) clock: SharedClock,
    policy: SessionPolicy,
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: Uuid,
    account_id: Uuid,
    scope: String,
    device_id: Option<Uuid>,
    access_expires_at: OffsetDateTime,
    idle_expires_at: OffsetDateTime,
    absolute_expires_at: OffsetDateTime,
    revoked_at: Option<OffsetDateTime>,
    status: String,
    account_created_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct RefreshRow {
    session_id: Uuid,
    generation: i32,
    consumed_at: Option<OffsetDateTime>,
    account_id: Uuid,
    revoked_at: Option<OffsetDateTime>,
    idle_expires_at: OffsetDateTime,
    absolute_expires_at: OffsetDateTime,
    access_token_hash: Vec<u8>,
    status: String,
}

impl IdentityService {
    pub fn new(
        pool: PgPool,
        redis: Option<ConnectionManager>,
        clock: SharedClock,
        policy: SessionPolicy,
    ) -> Self {
        Self {
            pool,
            cache: SessionCache::new(redis),
            clock,
            policy,
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// The clock this service uses (shared with other stores so tests drive one clock).
    pub fn clock(&self) -> SharedClock {
        std::sync::Arc::clone(&self.clock)
    }

    /// Returns the account of a verified identity, creating it (status `pending_age`) on first
    /// sign-in. `created` tells the caller whether this is a new account. `signals` (the region
    /// signals seen at sign-up, SR-24) are stored only on a new account.
    pub async fn find_or_create_account(
        &self,
        identity: &VerifiedIdentity,
        signals: &RegionSignals,
    ) -> Result<(Uuid, bool), StoreError> {
        if let Some(account_id) = self.find_account(identity).await? {
            return Ok((account_id, false));
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let Some(account_id) = insert_account(&mut tx, identity, signals, now).await? else {
            // A concurrent first sign-in won the race: drop ours and use theirs.
            tx.rollback().await?;
            let existing = self.find_account(identity).await?;
            return existing
                .map(|id| (id, false))
                .ok_or_else(|| StoreError::Corrupt("identity vanished during sign-in".to_owned()));
        };
        tx.commit().await?;
        Ok((account_id, true))
    }

    /// The account a verified provider identity belongs to, if any.
    pub async fn find_account(
        &self,
        identity: &VerifiedIdentity,
    ) -> Result<Option<Uuid>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT account_id FROM identity.auth_identities WHERE provider = $1 AND subject = $2",
        )
        .bind(identity.provider.as_str())
        .bind(&identity.subject)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Starts a new device session (= refresh family) for `account_id`.
    pub async fn start_session(
        &self,
        account_id: Uuid,
        device: Option<&DeviceInfo>,
    ) -> Result<IssuedSession, StoreError> {
        let now = self.clock.now();
        let status = self.account_status(account_id).await?;
        // A deletion-pending account signs in to a full session, which the access layer limits to
        // the routes that show and cancel the deletion (T-151, AC-29.2).
        let scope = if matches!(
            status,
            AccountStatus::Active | AccountStatus::DeletionPending
        ) {
            SessionScope::Full
        } else {
            SessionScope::Onboarding
        };
        let access = SecretToken::generate()?;
        let refresh = SecretToken::generate()?;
        let session_id = new_id();
        let access_expires_at = now + self.policy.access_ttl;
        let absolute = now + self.policy.absolute_ttl;
        let idle = (now + self.policy.idle_ttl).min(absolute);

        let mut tx = self.pool.begin().await?;
        if let Some(device) = device {
            sqlx::query(
                "INSERT INTO identity.devices (account_id, id, platform, app_version, last_seen_at, created_at)
                 VALUES ($1, $2, $3, $4, $5, $5)
                 ON CONFLICT (account_id, id) DO UPDATE
                 SET platform = EXCLUDED.platform, app_version = EXCLUDED.app_version, last_seen_at = EXCLUDED.last_seen_at",
            )
            .bind(account_id)
            .bind(device.id)
            .bind(device.platform.as_str())
            .bind(&device.app_version)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        let client_label = device.map(|d| format!("{} {}", d.platform.as_str(), d.app_version));
        sqlx::query(
            "INSERT INTO identity.sessions
               (id, account_id, access_token_hash, access_expires_at, scope, device_id, client_label,
                idle_expires_at, absolute_expires_at, last_seen_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10)",
        )
        .bind(session_id)
        .bind(account_id)
        .bind(access.hash().as_bytes())
        .bind(access_expires_at)
        .bind(scope.as_str())
        .bind(device.map(|d| d.id))
        .bind(client_label)
        .bind(idle)
        .bind(absolute)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        insert_refresh_token(&mut tx, &refresh, session_id, 1, now).await?;
        tx.commit().await?;

        Ok(IssuedSession {
            session_id,
            account_id,
            access_token: access,
            access_expires_at,
            refresh_token: refresh,
        })
    }

    /// The account with this id, if it still exists.
    pub async fn account(&self, account_id: Uuid) -> Result<Option<AccountSummary>, StoreError> {
        let row: Option<(String, sqlx::types::Json<RegionSignals>)> =
            sqlx::query_as("SELECT status, region_signals FROM identity.accounts WHERE id = $1")
                .bind(account_id)
                .fetch_optional(&self.pool)
                .await?;
        row.map(|(status, signals)| {
            Ok(AccountSummary {
                id: account_id,
                status: status.parse().map_err(|err: crate::model::UnknownValue| {
                    StoreError::Corrupt(err.to_string())
                })?,
                region_signals: signals.0,
            })
        })
        .transpose()
    }

    pub(crate) async fn account_status(
        &self,
        account_id: Uuid,
    ) -> Result<AccountStatus, StoreError> {
        let status: String =
            sqlx::query_scalar("SELECT status FROM identity.accounts WHERE id = $1")
                .bind(account_id)
                .fetch_one(&self.pool)
                .await?;
        status
            .parse()
            .map_err(|err: crate::model::UnknownValue| StoreError::Corrupt(err.to_string()))
    }

    /// Resolves an access token to its live session (cache first, then Postgres).
    pub async fn resolve_access(&self, raw_token: &str) -> Result<SessionPrincipal, AccessError> {
        let hash = TokenHash::of(raw_token);
        let now = self.clock.now();
        if let Some(principal) = self.cache.get(&hash).await {
            if now < principal.access_expires_at {
                return Ok(principal);
            }
            return Err(AccessError::Expired);
        }

        let row: Option<SessionRow> = sqlx::query_as(
            "SELECT s.id, s.account_id, s.scope, s.device_id, s.access_expires_at, s.idle_expires_at,
                    s.absolute_expires_at, s.revoked_at, a.status, a.created_at AS account_created_at
             FROM identity.sessions s JOIN identity.accounts a ON a.id = s.account_id
             WHERE s.access_token_hash = $1",
        )
        .bind(hash.as_bytes())
        .fetch_optional(&self.pool)
        .await?;
        let row = row.ok_or(AccessError::Unknown)?;
        if matches!(row.status.as_str(), "suspended" | "banned") {
            return Err(AccessError::Blocked);
        }
        if row.revoked_at.is_some() || now >= row.absolute_expires_at || now >= row.idle_expires_at
        {
            return Err(AccessError::Revoked);
        }
        if now >= row.access_expires_at {
            return Err(AccessError::Expired);
        }
        let principal = SessionPrincipal {
            account_id: row.account_id,
            session_id: row.id,
            scope: row.scope.parse().map_err(corrupt)?,
            status: row.status.parse().map_err(corrupt)?,
            account_created_at: row.account_created_at,
            device_id: row.device_id,
            access_expires_at: row.access_expires_at,
        };
        self.cache.put(&hash, &principal, now).await;
        Ok(principal)
    }

    /// Rotates a refresh token: consumes it and returns a new access + refresh pair for the same
    /// session. Replaying a consumed token revokes the session and returns [`RefreshError::Reused`].
    pub async fn refresh(&self, raw_refresh_token: &str) -> Result<IssuedSession, RefreshError> {
        let presented = TokenHash::of(raw_refresh_token);
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;

        // Lock the session row: concurrent refreshes of one family are serialized.
        let row: Option<RefreshRow> = sqlx::query_as(
            "SELECT rt.session_id, rt.generation, rt.consumed_at, s.account_id, s.revoked_at,
                    s.idle_expires_at, s.absolute_expires_at, s.access_token_hash, a.status
             FROM identity.refresh_tokens rt
             JOIN identity.sessions s ON s.id = rt.session_id
             JOIN identity.accounts a ON a.id = s.account_id
             WHERE rt.token_hash = $1
             FOR UPDATE OF s",
        )
        .bind(presented.as_bytes())
        .fetch_optional(&mut *tx)
        .await?;
        let row = row.ok_or(RefreshError::Unknown)?;
        let blocked = matches!(
            row.status.parse::<AccountStatus>(),
            Ok(AccountStatus::Suspended | AccountStatus::Banned) | Err(_)
        );
        if blocked
            || row.revoked_at.is_some()
            || now >= row.idle_expires_at
            || now >= row.absolute_expires_at
        {
            return Err(RefreshError::Revoked);
        }

        let latest: (Vec<u8>, i32, Option<OffsetDateTime>) = sqlx::query_as(
            "SELECT token_hash, generation, consumed_at FROM identity.refresh_tokens
             WHERE session_id = $1 ORDER BY generation DESC LIMIT 1",
        )
        .bind(row.session_id)
        .fetch_one(&mut *tx)
        .await?;
        let (latest_hash, latest_generation, latest_consumed) = latest;

        let to_consume = match row.consumed_at {
            None if row.generation == latest_generation => presented.as_bytes().to_vec(),
            Some(consumed_at)
                if row.generation == latest_generation - 1
                    && latest_consumed.is_none()
                    && now - consumed_at <= self.policy.refresh_grace =>
            {
                // Retry of the previous token: supersede the unused successor instead.
                latest_hash
            }
            _ => {
                let old_access = row.access_token_hash.clone();
                revoke_in_tx(&mut tx, row.session_id, RevokeReason::RefreshReuse, now).await?;
                let entry = AuditEntry::new(Actor::User(row.account_id), AuditAction::RefreshReuse)
                    .target("session", row.session_id);
                audit::record(&mut *tx, &entry, now)
                    .await
                    .map_err(StoreError::from)?;
                tx.commit().await?;
                self.cache.delete_raw(&old_access).await;
                tracing::warn!(
                    session_id = %row.session_id,
                    account_id = %row.account_id,
                    event = "auth.refresh_reuse",
                    "refresh token reuse detected; session family revoked"
                );
                return Err(RefreshError::Reused);
            }
        };

        sqlx::query("UPDATE identity.refresh_tokens SET consumed_at = $2 WHERE token_hash = $1")
            .bind(&to_consume)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        let access = SecretToken::generate().map_err(StoreError::from)?;
        let refresh = SecretToken::generate().map_err(StoreError::from)?;
        let access_expires_at = now + self.policy.access_ttl;
        let idle = (now + self.policy.idle_ttl).min(row.absolute_expires_at);
        sqlx::query(
            "UPDATE identity.sessions
             SET access_token_hash = $2, access_expires_at = $3, idle_expires_at = $4, last_seen_at = $5
             WHERE id = $1",
        )
        .bind(row.session_id)
        .bind(access.hash().as_bytes())
        .bind(access_expires_at)
        .bind(idle)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        insert_refresh_token(
            &mut tx,
            &refresh,
            row.session_id,
            latest_generation + 1,
            now,
        )
        .await?;
        tx.commit().await?;
        // The previous access token no longer matches the row; drop any cached copy.
        self.cache.delete_raw(&row.access_token_hash).await;

        Ok(IssuedSession {
            session_id: row.session_id,
            account_id: row.account_id,
            access_token: access,
            access_expires_at,
            refresh_token: refresh,
        })
    }

    /// Revokes one session (and its child widget/reauth sessions). Idempotent.
    pub async fn revoke_session(
        &self,
        session_id: Uuid,
        reason: RevokeReason,
    ) -> Result<(), StoreError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let hashes = revoke_in_tx(&mut tx, session_id, reason, now).await?;
        tx.commit().await?;
        for hash in hashes {
            self.cache.delete_raw(&hash).await;
        }
        Ok(())
    }

    /// Revokes every live session of an account ("log out everywhere", suspension, deletion).
    pub async fn revoke_all_sessions(
        &self,
        account_id: Uuid,
        reason: RevokeReason,
    ) -> Result<(), StoreError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let hashes: Vec<Vec<u8>> = sqlx::query_scalar(
            "UPDATE identity.sessions SET revoked_at = $2, revoked_reason = $3
             WHERE account_id = $1 AND revoked_at IS NULL
             RETURNING access_token_hash",
        )
        .bind(account_id)
        .bind(now)
        .bind(reason.as_str())
        .fetch_all(&mut *tx)
        .await?;
        if reason == RevokeReason::LogoutAll {
            let entry = AuditEntry::new(Actor::User(account_id), AuditAction::LogoutAll)
                .target("account", account_id)
                .detail("sessions", hashes.len());
            audit::record(&mut *tx, &entry, now).await?;
        }
        tx.commit().await?;
        for hash in hashes {
            self.cache.delete_raw(&hash).await;
        }
        Ok(())
    }

    /// Suspends an account (pending or active; banned and deleting accounts are left as they
    /// are) and revokes every live session. `reason` is a short code for the audit log.
    /// Returns `false` when nothing changed. Idempotent.
    pub async fn suspend_account(
        &self,
        account_id: Uuid,
        reason: &'static str,
    ) -> Result<bool, StoreError> {
        self.suspend_account_until(account_id, reason, None).await
    }

    /// Like [`Self::suspend_account`], ending at `until` (T-147; `None` = indefinite). The worker
    /// lifts it with [`Self::lift_expired_suspensions`]. Suspending an already suspended account
    /// replaces its end.
    pub async fn suspend_account_until(
        &self,
        account_id: Uuid,
        reason: &'static str,
        until: Option<OffsetDateTime>,
    ) -> Result<bool, StoreError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE identity.accounts SET suspended_until = $2 WHERE id = $1 AND status = 'suspended'",
        )
        .bind(account_id)
        .bind(until)
        .execute(&mut *tx)
        .await?;
        let suspended = sqlx::query(
            "UPDATE identity.accounts SET status = 'suspended', suspended_until = $3, updated_at = $2
             WHERE id = $1 AND status IN ('pending_age', 'pending_profile', 'active')",
        )
        .bind(account_id)
        .bind(now)
        .bind(until)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if suspended {
            let entry = AuditEntry::new(Actor::System, AuditAction::AccountSuspended)
                .target("account", account_id)
                .detail("reason", reason);
            audit::record(&mut *tx, &entry, now).await?;
        }
        tx.commit().await?;
        // Revoke even when the status was already suspended: no live session may survive.
        self.revoke_all_sessions(account_id, RevokeReason::Suspended)
            .await?;
        Ok(suspended)
    }

    /// Restores `active` for suspensions whose end passed (T-147). Accounts suspended
    /// indefinitely, banned or deleted are never touched. Returns how many were lifted.
    pub async fn lift_expired_suspensions(&self) -> Result<u64, StoreError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let lifted: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE identity.accounts SET status = 'active', suspended_until = NULL, updated_at = $1
             WHERE status = 'suspended' AND suspended_until IS NOT NULL AND suspended_until <= $1
             RETURNING id",
        )
        .bind(now)
        .fetch_all(&mut *tx)
        .await?;
        for id in &lifted {
            let entry = AuditEntry::new(Actor::System, AuditAction::AccountUnsuspended)
                .target("account", *id);
            audit::record(&mut *tx, &entry, now).await?;
        }
        tx.commit().await?;
        Ok(lifted.len() as u64)
    }

    /// Bans an account (from any status but deleting/deleted) and revokes every live session
    /// (T-146). Returns `false` when it was already banned. Idempotent.
    pub async fn ban_account(&self, account_id: Uuid) -> Result<bool, StoreError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let banned = sqlx::query(
            "UPDATE identity.accounts SET status = 'banned', suspended_until = NULL, updated_at = $2
             WHERE id = $1 AND status IN ('pending_age', 'pending_profile', 'active', 'suspended')",
        )
        .bind(account_id)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if banned {
            let entry = AuditEntry::new(Actor::System, AuditAction::AccountBanned)
                .target("account", account_id);
            audit::record(&mut *tx, &entry, now).await?;
        }
        tx.commit().await?;
        self.revoke_all_sessions(account_id, RevokeReason::Suspended)
            .await?;
        Ok(banned)
    }

    /// When the account was created, if it exists.
    pub async fn account_created_at(
        &self,
        account_id: Uuid,
    ) -> Result<Option<OffsetDateTime>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT created_at FROM identity.accounts WHERE id = $1")
                .bind(account_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Drops cached lookups for every live session of an account, so a status change (age gate,
    /// suspension) takes effect on the next request instead of after the cache TTL.
    pub async fn invalidate_cached_sessions(&self, account_id: Uuid) -> Result<(), StoreError> {
        let hashes: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT access_token_hash FROM identity.sessions WHERE account_id = $1 AND revoked_at IS NULL",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;
        for hash in hashes {
            self.cache.delete_raw(&hash).await;
        }
        Ok(())
    }
}

fn corrupt(err: crate::model::UnknownValue) -> AccessError {
    AccessError::Store(StoreError::Corrupt(err.to_string()))
}

/// Inserts a new account (status `pending_age`) and its identity in `tx`. `None` when the identity
/// already exists (a concurrent first sign-in won): the caller rolls back and uses that account.
pub(crate) async fn insert_account(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &VerifiedIdentity,
    signals: &RegionSignals,
    now: OffsetDateTime,
) -> Result<Option<Uuid>, sqlx::Error> {
    let account_id = new_id();
    sqlx::query(
        "INSERT INTO identity.accounts (id, status, region_signals, created_at, updated_at)
         VALUES ($1, 'pending_age', $3, $2, $2)",
    )
    .bind(account_id)
    .bind(now)
    .bind(sqlx::types::Json(signals))
    .execute(&mut **tx)
    .await?;
    let inserted = sqlx::query(
        "INSERT INTO identity.auth_identities (id, account_id, provider, subject, email, created_at)
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (provider, subject) DO NOTHING",
    )
    .bind(new_id())
    .bind(account_id)
    .bind(identity.provider.as_str())
    .bind(&identity.subject)
    .bind(identity.email.as_deref())
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok((inserted.rows_affected() == 1).then_some(account_id))
}

async fn insert_refresh_token(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    token: &SecretToken,
    session_id: Uuid,
    generation: i32,
    now: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO identity.refresh_tokens (token_hash, session_id, generation, issued_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(token.hash().as_bytes())
    .bind(session_id)
    .bind(generation)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Revokes a session and its children; returns their access-token hashes for cache eviction.
pub(crate) async fn revoke_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
    reason: RevokeReason,
    now: OffsetDateTime,
) -> Result<Vec<Vec<u8>>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE identity.sessions SET revoked_at = $2, revoked_reason = $3
         WHERE (id = $1 OR parent_session_id = $1) AND revoked_at IS NULL
         RETURNING access_token_hash",
    )
    .bind(session_id)
    .bind(now)
    .bind(reason.as_str())
    .fetch_all(&mut **tx)
    .await
}
