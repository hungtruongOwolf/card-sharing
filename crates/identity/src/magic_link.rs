//! Email magic links, start and delivery (T-22, US-1, SR-1, ADR-0004 AC-1.2/1.3). Verification
//! (the in-app confirm with the PKCE verifier) is T-23.
//!
//! - `start` never looks up whether the email belongs to an account: every well-formed request
//!   does the same work (one row, one job) and gets the same 202, so existing and unknown emails
//!   cannot be told apart by response or latency. The email is sent by a worker job, so the mail
//!   provider's latency never reaches the response either.
//! - The 256-bit token is stored only as SHA-256. Until delivery, the address and the raw token
//!   are kept AEAD-encrypted (bound to the link id) and cleared once the email is sent. Neither
//!   ever appears in logs, job payloads or outbox events.
//! - 15-minute lifetime, single use, bound to the device's PKCE S256 challenge.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use platform::crypto::FieldCipher;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob, enqueue};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::Duration;
use uuid::Uuid;

use crate::mail::{Email, MailError, Mailer, valid_address};
use crate::service::{IdentityService, StoreError};
use crate::token::{SecretToken, TokenHash};

/// Job kind that sends a magic-link email (payload: `{"magic_link_id": <uuid>}`).
pub const MAGIC_LINK_JOB: &str = "identity.magic_link.send";
const LINK_TTL: Duration = Duration::minutes(15);

#[derive(Debug, thiserror::Error)]
pub enum MagicLinkError {
    #[error("invalid email address")]
    InvalidEmail,
    #[error("invalid PKCE code challenge")]
    InvalidChallenge,
    #[error("encryption failed: {0}")]
    Crypto(#[from] platform::crypto::CryptoError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for MagicLinkError {
    fn from(err: sqlx::Error) -> Self {
        Self::Store(StoreError::Database(err))
    }
}

/// What the delivery job needs, encrypted at rest until it is sent.
#[derive(Serialize, Deserialize)]
struct Delivery {
    email: String,
    token: String,
}

/// Where links point and what the email says.
#[derive(Debug, Clone)]
pub struct MagicLinkTemplate {
    /// Universal/App Link base, e.g. `https://cardsharing.app/auth/link`. The token goes in the
    /// fragment, so it is never sent to the web server that hosts the fallback page.
    pub link_url: String,
}

/// Lowercased, trimmed email, or `None` when it is not a plausible address.
pub fn normalize_email(raw: &str) -> Option<String> {
    let email = raw.trim().to_ascii_lowercase();
    valid_address(&email).then_some(email)
}

/// SHA-256 of a normalized email (rate-limit key and account subject).
pub fn email_hash(normalized: &str) -> TokenHash {
    TokenHash::of(normalized)
}

/// The PKCE S256 challenge: base64url (no padding) of 32 bytes.
fn decode_challenge(challenge: &str) -> Option<[u8; 32]> {
    if challenge.len() != 43 {
        return None;
    }
    URL_SAFE_NO_PAD.decode(challenge).ok()?.try_into().ok()
}

impl IdentityService {
    /// Records a magic link for `email` and enqueues its delivery, in one transaction.
    pub async fn start_magic_link(
        &self,
        email: &str,
        code_challenge: &str,
        cipher: &FieldCipher,
    ) -> Result<(), MagicLinkError> {
        let email = normalize_email(email).ok_or(MagicLinkError::InvalidEmail)?;
        let challenge = decode_challenge(code_challenge).ok_or(MagicLinkError::InvalidChallenge)?;
        let token = SecretToken::generate().map_err(StoreError::from)?;
        let now = self.clock.now();
        let id = Uuid::now_v7();
        let delivery = serde_json::to_vec(&Delivery {
            email: email.clone(),
            token: token.expose().to_owned(),
        })
        .map_err(|err| StoreError::Corrupt(err.to_string()))?;
        let delivery_enc = cipher.encrypt(&delivery, id.as_bytes())?;

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO identity.magic_links
               (id, email_hash, token_hash, pkce_challenge, delivery_enc, expires_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(id)
        .bind(email_hash(&email).as_bytes())
        .bind(token.hash().as_bytes())
        .bind(&challenge[..])
        .bind(delivery_enc)
        .bind(now + LINK_TTL)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let job = NewJob::new(
            Lane::Critical,
            MAGIC_LINK_JOB,
            json!({ "magic_link_id": id }),
        )
        .unique(format!("magic_link:{id}"))
        .max_attempts(5);
        enqueue(&mut *tx, &job, now).await?;
        tx.commit().await?;
        tracing::info!(event = "auth.magic_link.started", magic_link_id = %id);
        Ok(())
    }

    /// Sends the email of a magic link and clears its encrypted delivery data. Idempotent: an
    /// already sent or purged link is a no-op. Expired links are not sent.
    pub async fn deliver_magic_link(
        &self,
        id: Uuid,
        cipher: &FieldCipher,
        mailer: &dyn Mailer,
        template: &MagicLinkTemplate,
    ) -> Result<(), DeliveryError> {
        let row: Option<(Option<Vec<u8>>, time::OffsetDateTime)> = sqlx::query_as(
            "SELECT delivery_enc, expires_at FROM identity.magic_links WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| DeliveryError::Temporary(err.to_string()))?;
        let Some((Some(sealed), expires_at)) = row else {
            return Ok(());
        };
        if self.clock.now() >= expires_at {
            self.clear_delivery(id).await?;
            tracing::warn!(event = "auth.magic_link.expired_unsent", magic_link_id = %id);
            return Ok(());
        }
        let plain = cipher
            .decrypt(&sealed, id.as_bytes())
            .map_err(|err| DeliveryError::Permanent(err.to_string()))?;
        let delivery: Delivery = serde_json::from_slice(&plain)
            .map_err(|err| DeliveryError::Permanent(err.to_string()))?;
        let email = Email {
            to: delivery.email,
            subject: "Your sign-in link".to_owned(),
            text: format!(
                "Tap this link on the phone where you asked to sign in:\n\n{}#token={}\n\n\
                 It works once and expires in 15 minutes. If you did not ask for it, ignore this email.\n",
                template.link_url, delivery.token
            ),
        };
        mailer.send(&email).await.map_err(|err| match err {
            MailError::Temporary(reason) => DeliveryError::Temporary(reason),
            MailError::Permanent(reason) => DeliveryError::Permanent(reason),
        })?;
        self.clear_delivery(id).await?;
        tracing::info!(event = "auth.magic_link.sent", magic_link_id = %id);
        Ok(())
    }

    async fn clear_delivery(&self, id: Uuid) -> Result<(), DeliveryError> {
        sqlx::query(
            "UPDATE identity.magic_links SET delivery_enc = NULL, sent_at = $2 WHERE id = $1",
        )
        .bind(id)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await
        .map_err(|err| DeliveryError::Temporary(err.to_string()))?;
        Ok(())
    }

    /// Deletes magic links older than 24 hours; returns how many.
    pub async fn purge_magic_links(&self) -> Result<u64, StoreError> {
        let result = sqlx::query("DELETE FROM identity.magic_links WHERE created_at <= $1")
            .bind(self.clock.now() - Duration::hours(24))
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

/// Delivery failure, mapped to the job queue's retry decision.
#[derive(Debug, thiserror::Error)]
pub enum DeliveryError {
    #[error("temporary: {0}")]
    Temporary(String),
    #[error("permanent: {0}")]
    Permanent(String),
}

/// Worker handler of [`MAGIC_LINK_JOB`].
pub struct MagicLinkJob {
    pub identity: Arc<IdentityService>,
    pub cipher: Arc<FieldCipher>,
    pub mailer: Arc<dyn Mailer>,
    pub template: MagicLinkTemplate,
}

#[derive(Deserialize)]
struct MagicLinkPayload {
    magic_link_id: Uuid,
}

impl JobHandler for MagicLinkJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let identity = Arc::clone(&self.identity);
        let cipher = Arc::clone(&self.cipher);
        let mailer = Arc::clone(&self.mailer);
        let template = self.template.clone();
        Box::pin(async move {
            let payload: MagicLinkPayload = serde_json::from_value(job.payload)
                .map_err(|err| JobError::Permanent(format!("payload: {err}")))?;
            identity
                .deliver_magic_link(payload.magic_link_id, &cipher, mailer.as_ref(), &template)
                .await
                .map_err(|err| match err {
                    DeliveryError::Temporary(reason) => JobError::Retry(reason),
                    DeliveryError::Permanent(reason) => JobError::Permanent(reason),
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_normalized_and_validated() {
        assert_eq!(
            normalize_email("  Cat@Example.COM ").as_deref(),
            Some("cat@example.com")
        );
        assert!(normalize_email("not an email").is_none());
        assert!(normalize_email("a@b.c\r\nBcc: x@y.z").is_none());
    }

    #[test]
    fn challenges_must_be_43_char_base64url() {
        let ok = URL_SAFE_NO_PAD.encode([7u8; 32]);
        assert!(decode_challenge(&ok).is_some());
        assert!(decode_challenge("short").is_none());
        assert!(decode_challenge(&"!".repeat(43)).is_none());
    }
}
