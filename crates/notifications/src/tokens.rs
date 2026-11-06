//! Push tokens (T-85, SR-15, SR-30). A token is registered by a device session for that session's
//! own device, encrypted at rest, unique across accounts (registering it on another account moves
//! it, so the previous account stops receiving), and only used while the registering session is
//! live: logout, "log out everywhere", suspension and deletion stop pushes immediately.

use std::sync::Arc;

use platform::clock::SharedClock;
use platform::crypto::FieldCipher;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Push provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Apns,
    Fcm,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Apns => "apns",
            Self::Fcm => "fcm",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "apns" => Some(Self::Apns),
            "fcm" => Some(Self::Fcm),
            _ => None,
        }
    }
}

/// What a token is for: user-visible alerts, or silent widget refreshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Alert,
    Widget,
}

impl TokenKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Alert => "alert",
            Self::Widget => "widget",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "alert" => Some(Self::Alert),
            "widget" => Some(Self::Widget),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    #[error("token is malformed for its provider")]
    Malformed,
}

/// A provider token, checked for shape. `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct PushToken(String);

impl PushToken {
    /// APNs device tokens are hex (32 bytes today; up to 100 bytes accepted); FCM registration
    /// tokens are URL-safe text up to 4096 characters.
    pub fn parse(provider: Provider, raw: &str) -> Result<Self, TokenError> {
        let ok = match provider {
            Provider::Apns => {
                (64..=200).contains(&raw.len())
                    && raw.len().is_multiple_of(2)
                    && raw.bytes().all(|b| b.is_ascii_hexdigit())
            }
            Provider::Fcm => {
                (1..=4096).contains(&raw.len())
                    && raw
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
            }
        };
        if !ok {
            return Err(TokenError::Malformed);
        }
        Ok(Self(match provider {
            Provider::Apns => raw.to_ascii_lowercase(),
            Provider::Fcm => raw.to_owned(),
        }))
    }

    /// The raw token, for the provider call only. Never log it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    fn hash(&self) -> Vec<u8> {
        Sha256::digest(self.0.as_bytes()).to_vec()
    }
}

impl std::fmt::Debug for PushToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PushToken(..)")
    }
}

/// Who registers: always the caller's own verified session and its device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registrant {
    pub account_id: Uuid,
    pub session_id: Uuid,
    pub device_id: Uuid,
}

/// A token a push may be sent to right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushTarget {
    pub id: Uuid,
    pub device_id: Uuid,
    pub provider: Provider,
    pub kind: TokenKind,
    pub token: PushToken,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("stored token could not be decrypted or parsed")]
    Corrupt,
}

/// Postgres-backed push token registry (`notifications.push_tokens`).
#[derive(Clone)]
pub struct PushTokens {
    pool: PgPool,
    clock: SharedClock,
    cipher: Arc<FieldCipher>,
}

impl PushTokens {
    pub fn new(pool: PgPool, clock: SharedClock, cipher: Arc<FieldCipher>) -> Self {
        Self {
            pool,
            clock,
            cipher,
        }
    }

    /// Registers (or replaces) the token of `kind` for the registrant's device. The same token
    /// held by any other row (another account, device or kind) is removed in the same
    /// transaction, so it is never delivered to two places.
    pub async fn register(
        &self,
        who: Registrant,
        provider: Provider,
        kind: TokenKind,
        token: &PushToken,
    ) -> Result<(), StoreError> {
        let now = self.clock.now();
        let hash = token.hash();
        let enc = self
            .cipher
            .encrypt(token.expose().as_bytes(), who.account_id.as_bytes())
            .map_err(|_| StoreError::Corrupt)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM notifications.push_tokens
             WHERE token_hash = $1 AND NOT (account_id = $2 AND device_id = $3 AND kind = $4)",
        )
        .bind(&hash)
        .bind(who.account_id)
        .bind(who.device_id)
        .bind(kind.as_str())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO notifications.push_tokens
                 (id, account_id, device_id, session_id, provider, kind, token_enc, token_hash,
                  created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
             ON CONFLICT (account_id, device_id, kind) DO UPDATE
             SET session_id = EXCLUDED.session_id, provider = EXCLUDED.provider,
                 token_enc = EXCLUDED.token_enc, token_hash = EXCLUDED.token_hash,
                 updated_at = EXCLUDED.updated_at, invalidated_at = NULL",
        )
        .bind(Uuid::now_v7())
        .bind(who.account_id)
        .bind(who.device_id)
        .bind(who.session_id)
        .bind(provider.as_str())
        .bind(kind.as_str())
        .bind(enc)
        .bind(&hash)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Removes every token of the account's device. Returns how many were removed.
    pub async fn remove_device(
        &self,
        account_id: Uuid,
        device_id: Uuid,
    ) -> Result<u64, StoreError> {
        let done = sqlx::query(
            "DELETE FROM notifications.push_tokens WHERE account_id = $1 AND device_id = $2",
        )
        .bind(account_id)
        .bind(device_id)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// Tokens of `account_id` that may receive a push now: not invalidated, registering session
    /// live, account active.
    pub async fn live_targets(
        &self,
        account_id: Uuid,
        kind: TokenKind,
    ) -> Result<Vec<PushTarget>, StoreError> {
        let now = self.clock.now();
        let rows: Vec<(Uuid, Uuid, String, String, Vec<u8>)> = sqlx::query_as(concat!(
            "SELECT t.id, t.device_id, t.provider, t.kind, t.token_enc
               FROM notifications.push_tokens t
              WHERE t.account_id = $1 AND t.kind = $2 AND t.invalidated_at IS NULL AND ",
            identity::session_live_sql!("t.session_id", "$3"),
            " AND ",
            identity::account_active_sql!("t.account_id"),
            " ORDER BY t.id"
        ))
        .bind(account_id)
        .bind(kind.as_str())
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(id, device_id, provider, kind, enc)| {
                let provider = Provider::parse(&provider).ok_or(StoreError::Corrupt)?;
                let kind = TokenKind::parse(&kind).ok_or(StoreError::Corrupt)?;
                let raw = self
                    .cipher
                    .decrypt(&enc, account_id.as_bytes())
                    .map_err(|_| StoreError::Corrupt)?;
                let raw = String::from_utf8(raw).map_err(|_| StoreError::Corrupt)?;
                let token = PushToken::parse(provider, &raw).map_err(|_| StoreError::Corrupt)?;
                Ok(PushTarget {
                    id,
                    device_id,
                    provider,
                    kind,
                    token,
                })
            })
            .collect()
    }

    /// Marks a token the provider rejected as unregistered/invalid; it is never used again and
    /// the next prune deletes it.
    pub async fn invalidate(&self, token_id: Uuid) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE notifications.push_tokens SET invalidated_at = $2
             WHERE id = $1 AND invalidated_at IS NULL",
        )
        .bind(token_id)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Tokens a provider rejected since `since` (T-97 invalid-token rate).
    pub async fn invalidated_since(&self, since: OffsetDateTime) -> Result<i64, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM notifications.push_tokens WHERE invalidated_at >= $1",
        )
        .bind(since)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Deletes invalidated tokens and tokens whose session is no longer live (logout, expiry,
    /// suspension, deletion). Returns how many rows were deleted.
    pub async fn prune(&self) -> Result<u64, StoreError> {
        let now: OffsetDateTime = self.clock.now();
        let done = sqlx::query(concat!(
            "DELETE FROM notifications.push_tokens t
              WHERE t.invalidated_at IS NOT NULL OR NOT ",
            identity::session_live_sql!("t.session_id", "$1")
        ))
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn tokens_are_checked_per_provider_and_never_printed() {
        let apns = "AB".repeat(32);
        let token = PushToken::parse(Provider::Apns, &apns).unwrap();
        assert_eq!(token.expose(), "ab".repeat(32));
        assert_eq!(format!("{token:?}"), "PushToken(..)");
        for bad in ["", "zz", &"a".repeat(63), &"g".repeat(64), &"a".repeat(202)] {
            assert!(PushToken::parse(Provider::Apns, bad).is_err(), "{bad}");
        }
        assert!(PushToken::parse(Provider::Fcm, "dGVzdA:APA91b-x_y").is_ok());
        for bad in ["", "a b", "a/b", "a\nb", &"a".repeat(4097)] {
            assert!(PushToken::parse(Provider::Fcm, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn enums_round_trip() {
        for p in [Provider::Apns, Provider::Fcm] {
            assert_eq!(Provider::parse(p.as_str()), Some(p));
        }
        for k in [TokenKind::Alert, TokenKind::Widget] {
            assert_eq!(TokenKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(Provider::parse("APNS"), None);
        assert_eq!(TokenKind::parse("silent"), None);
    }
}
