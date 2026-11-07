//! Email magic links: start and delivery (T-22), verification (T-23); US-1, SR-1, ADR-0004
//! AC-1.2/1.3, ADR-0012.
//!
//! - `start` never looks up whether the email belongs to an account: every well-formed request
//!   does the same work (one row, one job) and gets the same 202 with a fresh request id, so
//!   existing and unknown emails cannot be told apart by response or latency. The email is sent by
//!   a worker job, so the mail provider's latency never reaches the response either.
//! - The 256-bit token is stored only as SHA-256. Until delivery, the address and the raw token
//!   are kept AEAD-encrypted (bound to the link id) and cleared once the email is sent. Neither
//!   ever appears in logs, job payloads or outbox events.
//! - The link is `<link base>#rid=<request id>&token=<token>`: everything after `#` stays in the
//!   browser (never sent to the web host). The app keeps the PKCE verifier under the request id.
//! - Verify needs the token and the verifier of the device that asked (S256, compared in constant
//!   time), within 15 minutes, once. An unknown token and a wrong verifier get the same answer;
//!   "expired" and "used" are told only to the holder of the verifier. A wrong verifier does not
//!   burn the link, so a forwarded copy cannot spoil the real one.
//! - Re-auth links (ADR-0012): a signed-in session asks for a link to the address of its account's
//!   own email identity (typed again; the address is not stored). The link is bound to that
//!   account and session, carries `&for=reauth`, and redeems to nothing but proof of a fresh
//!   sign-in for the re-auth token of that session. An address that is not the account's gets
//!   the same answer and no email.
//! - Behind the sign-up gate (invite_codes.rs) a link that would create an account needs an
//!   invite code. A missing or wrong code leaves the link usable, so the user can type the code
//!   and retry; wrong codes per link are capped, then the link expires.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use platform::crypto::FieldCipher;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob, enqueue};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::Duration;
use uuid::Uuid;

use platform::audit::{self, Actor, AuditAction, AuditEntry};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::invite_codes::{
    SignupGate, audit_redemption, normalize_invite_code, redeem_invite_code,
};
use crate::mail::{Email, MailError, Mailer, valid_address};
use crate::model::{AccountStatus, RegionSignals};
use crate::service::{
    DeviceInfo, IdentityService, Provider, StoreError, VerifiedIdentity, insert_account,
};
use crate::sign_in::SignedIn;
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
    /// The app's landing route, e.g. `https://cardsharing.app/auth/link` (Universal/App Link) or,
    /// while testers use the web target, `http://<pc-lan-ip>:8081/auth/link` (ADR-0012). The
    /// request id and the token go in the fragment, so they never reach the web host.
    pub link_url: String,
}

impl MagicLinkTemplate {
    /// The link of one request: `<link_url>#rid=<request id>&token=<token>`.
    pub fn link(&self, request_id: Uuid, token: &str) -> String {
        format!("{}#rid={request_id}&token={token}", self.link_url)
    }

    /// A re-auth link: the same, plus `&for=reauth` so the app hands it to the tab that asked.
    pub fn reauth_link(&self, request_id: Uuid, token: &str) -> String {
        format!("{}&for=reauth", self.link(request_id, token))
    }
}

/// What a link is for (`magic_links.purpose`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    SignIn,
    Reauth,
}

impl Purpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::SignIn => "sign_in",
            Self::Reauth => "reauth",
        }
    }
}

/// Why a re-auth link did not confirm the session.
#[derive(Debug, thiserror::Error)]
pub enum ReauthLinkError {
    /// Unknown token, another device's verifier, a sign-in link, or a link of another account or
    /// session. The caller cannot tell which.
    #[error("unknown link or verifier mismatch")]
    Invalid,
    #[error("link expired")]
    Expired,
    #[error("link already used")]
    Used,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for ReauthLinkError {
    fn from(err: sqlx::Error) -> Self {
        Self::Store(StoreError::Database(err))
    }
}

/// Why a sign-in link did not sign anyone in.
#[derive(Debug, thiserror::Error)]
pub enum LinkSignInError {
    /// Unknown token, or a verifier that does not match it (forwarded link, other browser or
    /// device). The caller cannot tell which.
    #[error("unknown link or verifier mismatch")]
    Invalid,
    /// Past its lifetime (only reported after the verifier matched).
    #[error("link expired")]
    Expired,
    /// Already used (only reported after the verifier matched).
    #[error("link already used")]
    Used,
    /// The account may not sign in (suspended or banned). The link is used up.
    #[error("account blocked: {0}")]
    Blocked(AccountStatus),
    /// The link would create an account and the sign-up gate needs a code; none was sent. The
    /// link is still usable.
    #[error("invite code required")]
    InviteRequired,
    /// The invite code is unknown, malformed, expired, revoked or used up (one answer). The link
    /// is still usable until the wrong-code cap, then it expires.
    #[error("invite code invalid")]
    InviteInvalid,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for LinkSignInError {
    fn from(err: sqlx::Error) -> Self {
        Self::Store(StoreError::Database(err))
    }
}

/// A PKCE code verifier (RFC 7636 section 4.1): 43 to 128 characters of `A-Z a-z 0-9 - . _ ~`.
pub fn valid_code_verifier(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

/// A link token as sent in the email: 43 base64url characters (256 bits).
pub fn valid_link_token(token: &str) -> bool {
    token.len() == 43
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Whether `SHA-256(verifier)` equals the stored S256 challenge, compared in constant time.
fn verifier_matches(challenge: &[u8], verifier: &str) -> bool {
    let digest: [u8; 32] = Sha256::digest(verifier.as_bytes()).into();
    if challenge.len() != digest.len() {
        return false;
    }
    let diff = challenge
        .iter()
        .zip(digest.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    std::hint::black_box(diff) == 0
}

/// The identity subject of an email account: lowercase hex of the email hash (accounts are keyed
/// by provider + subject; the address itself is not stored).
fn email_subject(email_hash: &[u8]) -> Result<String, StoreError> {
    let bytes: [u8; 32] = email_hash
        .try_into()
        .map_err(|_| StoreError::Corrupt("magic link email hash is not 32 bytes".to_owned()))?;
    Ok(TokenHash(bytes).to_hex())
}

#[derive(sqlx::FromRow)]
struct LinkRow {
    id: Uuid,
    email_hash: Vec<u8>,
    pkce_challenge: Vec<u8>,
    expires_at: OffsetDateTime,
    used_at: Option<OffsetDateTime>,
    purpose: String,
}

#[derive(sqlx::FromRow)]
struct ReauthLinkRow {
    id: Uuid,
    pkce_challenge: Vec<u8>,
    expires_at: OffsetDateTime,
    used_at: Option<OffsetDateTime>,
    purpose: String,
    account_id: Option<Uuid>,
    session_id: Option<Uuid>,
}

/// A redeemed link: the account it belongs to, and whether redeeming it created that account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Redeemed {
    account_id: Uuid,
    created: bool,
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
    /// Records a magic link for `email` and enqueues its delivery, in one transaction. Returns the
    /// request id (the link's id): the app keeps its PKCE verifier under it, and the emailed link
    /// carries it back.
    pub async fn start_magic_link(
        &self,
        email: &str,
        code_challenge: &str,
        cipher: &FieldCipher,
    ) -> Result<Uuid, MagicLinkError> {
        let email = normalize_email(email).ok_or(MagicLinkError::InvalidEmail)?;
        let challenge = decode_challenge(code_challenge).ok_or(MagicLinkError::InvalidChallenge)?;
        self.insert_link(&email, &challenge, cipher, None).await
    }

    /// Records a re-auth link for the signed-in `session_id` of `account_id` when `email` is the
    /// address of the account's email identity, and enqueues its delivery. Returns a request id
    /// either way (a fresh one, with nothing stored or sent, when the address is not the
    /// account's), so the answer does not say whether the typed address matched.
    pub async fn start_reauth_link(
        &self,
        account_id: Uuid,
        session_id: Uuid,
        email: &str,
        code_challenge: &str,
        cipher: &FieldCipher,
    ) -> Result<Uuid, MagicLinkError> {
        let email = normalize_email(email).ok_or(MagicLinkError::InvalidEmail)?;
        let challenge = decode_challenge(code_challenge).ok_or(MagicLinkError::InvalidChallenge)?;
        let owns: Option<Uuid> = sqlx::query_scalar(
            "SELECT account_id FROM identity.auth_identities
             WHERE account_id = $1 AND provider = $2 AND subject = $3",
        )
        .bind(account_id)
        .bind(Provider::Email.as_str())
        .bind(email_hash(&email).to_hex())
        .fetch_optional(&self.pool)
        .await?;
        if owns.is_none() {
            tracing::info!(event = "auth.reauth_link.not_sent", account_id = %account_id);
            return Ok(Uuid::now_v7());
        }
        self.insert_link(&email, &challenge, cipher, Some((account_id, session_id)))
            .await
    }

    /// Inserts a link (a re-auth link when `binding` names its account and session) and enqueues
    /// its delivery, in one transaction.
    async fn insert_link(
        &self,
        email: &str,
        challenge: &[u8; 32],
        cipher: &FieldCipher,
        binding: Option<(Uuid, Uuid)>,
    ) -> Result<Uuid, MagicLinkError> {
        let token = SecretToken::generate().map_err(StoreError::from)?;
        let now = self.clock.now();
        let id = Uuid::now_v7();
        let delivery = serde_json::to_vec(&Delivery {
            email: email.to_owned(),
            token: token.expose().to_owned(),
        })
        .map_err(|err| StoreError::Corrupt(err.to_string()))?;
        let delivery_enc = cipher.encrypt(&delivery, id.as_bytes())?;

        let mut tx = self.pool.begin().await?;
        let purpose = if binding.is_some() {
            Purpose::Reauth
        } else {
            Purpose::SignIn
        };
        sqlx::query(
            "INSERT INTO identity.magic_links
               (id, email_hash, token_hash, pkce_challenge, delivery_enc, expires_at, created_at,
                purpose, account_id, session_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(id)
        .bind(email_hash(email).as_bytes())
        .bind(token.hash().as_bytes())
        .bind(&challenge[..])
        .bind(delivery_enc)
        .bind(now + LINK_TTL)
        .bind(now)
        .bind(purpose.as_str())
        .bind(binding.map(|(account, _)| account))
        .bind(binding.map(|(_, session)| session))
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
        tracing::info!(
            event = "auth.magic_link.started",
            magic_link_id = %id,
            purpose = purpose.as_str()
        );
        Ok(id)
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
        let row: Option<(Option<Vec<u8>>, time::OffsetDateTime, String)> = sqlx::query_as(
            "SELECT delivery_enc, expires_at, purpose FROM identity.magic_links WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| DeliveryError::Temporary(err.to_string()))?;
        let Some((Some(sealed), expires_at, purpose)) = row else {
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
        let email = if purpose == Purpose::Reauth.as_str() {
            Email {
                to: delivery.email,
                subject: "Confirm it's you".to_owned(),
                text: format!(
                    "Open this link in the browser where you asked to confirm it's you, while that tab is still open:\n\n{}\n\n\
                     It works once and expires in 15 minutes. If you did not ask for it, someone may be using your \
                     account: sign out of every device from Settings.\n",
                    template.reauth_link(id, &delivery.token)
                ),
            }
        } else {
            Email {
                to: delivery.email,
                subject: "Your sign-in link".to_owned(),
                text: format!(
                    "Open this link on the device and in the browser where you asked to sign in:\n\n{}\n\n\
                     It works once and expires in 15 minutes. If you did not ask for it, ignore this email.\n",
                    template.link(id, &delivery.token)
                ),
            }
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

    /// Signs in with a magic link (T-23): redeems the link with the verifier of the device that
    /// asked for it, finds the email account or creates it behind the sign-up `gate`
    /// (`pending_age`, AC-1.1; never linked to an Apple/Google account with the same address,
    /// AC-1.3) and starts a session, exactly as a provider sign-in does (onboarding or full scope,
    /// audit `auth.sign_in`).
    pub async fn sign_in_with_magic_link(
        &self,
        token: &str,
        verifier: &str,
        device: Option<&DeviceInfo>,
        signals: &RegionSignals,
        gate: &SignupGate,
    ) -> Result<SignedIn, LinkSignInError> {
        if !valid_link_token(token) || !valid_code_verifier(verifier) {
            return Err(LinkSignInError::Invalid);
        }
        let token_hash = TokenHash::of(token);
        // A concurrent first sign-in with another link for the same email can win the account
        // insert; the second pass then finds that account.
        let mut redeemed = None;
        for _ in 0..2 {
            redeemed = self
                .redeem_sign_in_link(&token_hash, verifier, signals, gate)
                .await?;
            if redeemed.is_some() {
                break;
            }
        }
        let Redeemed {
            account_id,
            created,
        } = redeemed.ok_or_else(|| {
            StoreError::Corrupt("email account insert lost the race twice".to_owned())
        })?;
        let status = self.account_status(account_id).await?;
        if matches!(status, AccountStatus::Suspended | AccountStatus::Banned) {
            tracing::info!(event = "auth.magic_link.blocked", account_id = %account_id, %status);
            return Err(LinkSignInError::Blocked(status));
        }
        let session = self.start_session(account_id, device).await?;
        let entry = AuditEntry::new(Actor::User(account_id), AuditAction::SignIn)
            .target("session", session.session_id)
            .detail("provider", Provider::Email.as_str())
            .detail("new_account", created);
        audit::record_or_log(&self.pool, &entry, self.clock.now()).await;
        tracing::info!(
            event = "auth.sign_in",
            provider = Provider::Email.as_str(),
            account_id = %account_id,
            created,
            "signed in"
        );
        Ok(SignedIn { session, created })
    }

    /// One attempt at redeeming a sign-in link, in one transaction that locks the link row: checks
    /// the verifier first, then single use and lifetime, then finds the account or creates it
    /// behind the gate (spending one invite use in the same transaction) and marks the link used.
    /// `Ok(None)` = the account insert lost a race (nothing changed; retry).
    async fn redeem_sign_in_link(
        &self,
        token_hash: &TokenHash,
        verifier: &str,
        signals: &RegionSignals,
        gate: &SignupGate,
    ) -> Result<Option<Redeemed>, LinkSignInError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let link: Option<LinkRow> = sqlx::query_as(
            "SELECT id, email_hash, pkce_challenge, expires_at, used_at, purpose
             FROM identity.magic_links WHERE token_hash = $1 FOR UPDATE",
        )
        .bind(token_hash.as_bytes())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(link) = link else {
            tracing::info!(event = "auth.magic_link.rejected", reason = "unknown");
            return Err(LinkSignInError::Invalid);
        };
        if !verifier_matches(&link.pkce_challenge, verifier) {
            tracing::warn!(
                event = "auth.magic_link.rejected",
                reason = "verifier",
                magic_link_id = %link.id
            );
            return Err(LinkSignInError::Invalid);
        }
        if link.purpose != Purpose::SignIn.as_str() {
            // A re-auth link never signs anyone in.
            tracing::warn!(
                event = "auth.magic_link.rejected",
                reason = "purpose",
                magic_link_id = %link.id
            );
            return Err(LinkSignInError::Invalid);
        }
        if link.used_at.is_some() {
            tracing::info!(
                event = "auth.magic_link.rejected",
                reason = "used",
                magic_link_id = %link.id
            );
            return Err(LinkSignInError::Used);
        }
        if now >= link.expires_at {
            tracing::info!(
                event = "auth.magic_link.rejected",
                reason = "expired",
                magic_link_id = %link.id
            );
            return Err(LinkSignInError::Expired);
        }
        let identity = VerifiedIdentity {
            provider: Provider::Email,
            subject: email_subject(&link.email_hash)?,
            email: None,
        };
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT account_id FROM identity.auth_identities WHERE provider = $1 AND subject = $2",
        )
        .bind(identity.provider.as_str())
        .bind(&identity.subject)
        .fetch_optional(&mut *tx)
        .await?;
        let redeemed = match existing {
            Some(account_id) => Redeemed {
                account_id,
                created: false,
            },
            None => {
                let code_id = if gate.invite_required {
                    let Some(raw) = gate.invite_code.as_deref() else {
                        // Nothing changed: the link stays usable for the retry with a code.
                        return Err(LinkSignInError::InviteRequired);
                    };
                    let code_id = match normalize_invite_code(raw) {
                        Some(code) => redeem_invite_code(&mut tx, &code, now).await?,
                        None => None,
                    };
                    let Some(code_id) = code_id else {
                        self.count_wrong_invite(&mut tx, link.id, gate, now).await?;
                        tx.commit().await?;
                        return Err(LinkSignInError::InviteInvalid);
                    };
                    Some(code_id)
                } else {
                    None
                };
                let Some(account_id) = insert_account(&mut tx, &identity, signals, now).await?
                else {
                    tx.rollback().await?;
                    return Ok(None);
                };
                if let Some(code_id) = code_id {
                    audit_redemption(&mut tx, account_id, code_id, Provider::Email.as_str(), now)
                        .await?;
                }
                Redeemed {
                    account_id,
                    created: true,
                }
            }
        };
        sqlx::query("UPDATE identity.magic_links SET used_at = $2 WHERE id = $1")
            .bind(link.id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(
            event = "auth.magic_link.redeemed",
            magic_link_id = %link.id,
            created = redeemed.created
        );
        Ok(Some(redeemed))
    }

    /// Redeems a re-auth link for the calling `session_id` of `account_id` (ADR-0012): the verifier
    /// of the tab that asked, a link of purpose `reauth` bound to this account and session, not
    /// used, not expired. Marks it used; the caller then mints the re-auth token.
    pub async fn redeem_reauth_link(
        &self,
        account_id: Uuid,
        session_id: Uuid,
        token: &str,
        verifier: &str,
    ) -> Result<(), ReauthLinkError> {
        if !valid_link_token(token) || !valid_code_verifier(verifier) {
            return Err(ReauthLinkError::Invalid);
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let row: Option<ReauthLinkRow> = sqlx::query_as(
            "SELECT id, pkce_challenge, expires_at, used_at, purpose, account_id, session_id
             FROM identity.magic_links WHERE token_hash = $1 FOR UPDATE",
        )
        .bind(TokenHash::of(token).as_bytes())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(link) = row else {
            return Err(ReauthLinkError::Invalid);
        };
        if !verifier_matches(&link.pkce_challenge, verifier)
            || link.purpose != Purpose::Reauth.as_str()
            || link.account_id != Some(account_id)
            || link.session_id != Some(session_id)
        {
            tracing::warn!(event = "auth.reauth_link.rejected", magic_link_id = %link.id);
            return Err(ReauthLinkError::Invalid);
        }
        if link.used_at.is_some() {
            return Err(ReauthLinkError::Used);
        }
        if now >= link.expires_at {
            return Err(ReauthLinkError::Expired);
        }
        sqlx::query("UPDATE identity.magic_links SET used_at = $2 WHERE id = $1")
            .bind(link.id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(event = "auth.reauth_link.redeemed", magic_link_id = %link.id);
        Ok(())
    }

    /// Counts a wrong invite code against a link; the attempt that reaches the cap expires it.
    async fn count_wrong_invite(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        link_id: Uuid,
        gate: &SignupGate,
        now: OffsetDateTime,
    ) -> Result<(), sqlx::Error> {
        let expired: bool = sqlx::query_scalar(
            "UPDATE identity.magic_links
             SET invite_attempts = invite_attempts + 1,
                 expires_at = CASE WHEN invite_attempts + 1 >= $2 THEN LEAST(expires_at, $3)
                                   ELSE expires_at END
             WHERE id = $1
             RETURNING expires_at <= $3",
        )
        .bind(link_id)
        .bind(i32::from(gate.max_attempts_per_link))
        .bind(now)
        .fetch_one(&mut **tx)
        .await?;
        tracing::info!(
            event = "auth.magic_link.invite_rejected",
            magic_link_id = %link_id,
            link_expired = expired
        );
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
    fn verifiers_follow_rfc_7636() {
        assert!(valid_code_verifier(&"a".repeat(43)));
        assert!(valid_code_verifier(&"Az09-._~".repeat(16)));
        assert!(!valid_code_verifier(&"a".repeat(42)));
        assert!(!valid_code_verifier(&"a".repeat(129)));
        assert!(!valid_code_verifier(&format!("{}+", "a".repeat(42))));
    }

    #[test]
    fn tokens_are_43_char_base64url() {
        let token = SecretToken::generate().unwrap();
        assert!(valid_link_token(token.expose()));
        assert!(!valid_link_token(&"a".repeat(42)));
        assert!(!valid_link_token(&format!("{}=", "a".repeat(42))));
    }

    #[test]
    fn the_verifier_must_hash_to_the_challenge() {
        // RFC 7636 appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = decode_challenge("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM").unwrap();
        assert!(verifier_matches(&challenge, verifier));
        assert!(!verifier_matches(&challenge, &verifier.replace('d', "e")));
        assert!(!verifier_matches(&challenge[..31], verifier));
    }

    #[test]
    fn links_carry_the_request_id_and_token_in_the_fragment() {
        let template = MagicLinkTemplate {
            link_url: "http://192.168.1.20:8081/auth/link".to_owned(),
        };
        assert_eq!(
            template.link(Uuid::nil(), "tok"),
            "http://192.168.1.20:8081/auth/link#rid=00000000-0000-0000-0000-000000000000&token=tok"
        );
    }

    #[test]
    fn reauth_links_say_what_they_are_for() {
        let template = MagicLinkTemplate {
            link_url: "https://cardsharing.app/auth/link".to_owned(),
        };
        assert_eq!(
            template.reauth_link(Uuid::nil(), "tok"),
            "https://cardsharing.app/auth/link#rid=00000000-0000-0000-0000-000000000000&token=tok&for=reauth"
        );
    }

    #[test]
    fn challenges_must_be_43_char_base64url() {
        let ok = URL_SAFE_NO_PAD.encode([7u8; 32]);
        assert!(decode_challenge(&ok).is_some());
        assert!(decode_challenge("short").is_none());
        assert!(decode_challenge(&"!".repeat(43)).is_none());
    }
}
