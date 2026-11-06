//! Sign-in with a provider ID token (Apple T-19, Google T-20): verify → reject a replayed nonce →
//! find or create the account (keyed by provider + `sub` only; no linking by email, AC-1.3) →
//! (Apple) exchange the authorization code and store the refresh token encrypted → start an
//! onboarding or full session.

use std::collections::HashMap;
use std::sync::Arc;

use platform::audit::{self, Actor, AuditAction, AuditEntry};
use platform::crypto::FieldCipher;
use redis::AsyncCommands;
use redis::SetExpiry;
use redis::SetOptions;
use uuid::Uuid;

use crate::apple::{AppleTokenClient, AppleTokenError};
use crate::model::{AccountStatus, RegionSignals};
use crate::oidc::{IdTokenVerifier, VerifiedIdToken, VerifyError};
use crate::service::{
    DeviceInfo, IdentityService, IssuedSession, Provider, StoreError, VerifiedIdentity,
};

/// Upper bound for how long a used nonce is remembered. ID tokens live 10 min (Apple) to 1 h
/// (Google); a nonce only needs to be remembered while a token carrying it is still accepted.
const MAX_NONCE_TTL_SECS: i64 = 2 * 60 * 60;

/// One sign-in attempt, as received from the app.
#[derive(Debug, Clone)]
pub struct SignInRequest {
    pub id_token: String,
    /// The raw nonce the app generated; the token's `nonce` claim must be its SHA-256 (hex).
    pub raw_nonce: String,
    /// Apple only: one-time authorization code, exchanged for a refresh token.
    pub authorization_code: Option<String>,
    pub device: Option<DeviceInfo>,
    /// Region signals stored on a newly created account (SR-24).
    pub signals: RegionSignals,
}

#[derive(Debug)]
pub struct SignedIn {
    pub session: IssuedSession,
    /// A new account was created by this sign-in.
    pub created: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SignInError {
    /// Bad, expired, forged or misaddressed token, or a nonce mismatch. Clients get a uniform 401.
    #[error("rejected: {0}")]
    Rejected(&'static str),
    /// The nonce was already used by an accepted sign-in.
    #[error("nonce replayed")]
    Replayed,
    /// The account exists but may not sign in (suspended, banned). A deletion-pending account may
    /// sign in during its grace period to cancel the deletion (T-151, AC-29.2).
    #[error("account blocked: {0}")]
    Blocked(AccountStatus),
    /// The provider is not configured, its keys are unreachable, or the nonce store is down
    /// (fails closed).
    #[error("unavailable: {0}")]
    Unavailable(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct ProviderSignIn {
    identity: Arc<IdentityService>,
    verifiers: HashMap<Provider, IdTokenVerifier>,
    apple_tokens: Arc<dyn AppleTokenClient>,
    cipher: Arc<FieldCipher>,
}

impl ProviderSignIn {
    pub fn new(
        identity: Arc<IdentityService>,
        apple_tokens: Arc<dyn AppleTokenClient>,
        cipher: Arc<FieldCipher>,
    ) -> Self {
        Self {
            identity,
            verifiers: HashMap::new(),
            apple_tokens,
            cipher,
        }
    }

    /// Enables a provider. A provider without a verifier answers `Unavailable`.
    pub fn with_verifier(mut self, verifier: IdTokenVerifier) -> Self {
        self.verifiers.insert(verifier.provider(), verifier);
        self
    }

    /// Verifies a fresh provider proof (ID token + its raw nonce, single use) for re-auth (T-26)
    /// and returns the account it belongs to; `None` when no account has this identity. Never
    /// creates an account.
    pub async fn verify_proof(
        &self,
        provider: Provider,
        id_token: &str,
        raw_nonce: &str,
    ) -> Result<Option<Uuid>, SignInError> {
        let verifier = self.verifiers.get(&provider).ok_or_else(|| {
            SignInError::Unavailable(format!("{} not enabled", provider.as_str()))
        })?;
        let verified = verifier
            .verify(id_token, raw_nonce)
            .await
            .map_err(|err| match err {
                VerifyError::Invalid(reason) => SignInError::Rejected(reason),
                VerifyError::NotConfigured => {
                    SignInError::Unavailable(format!("{} not configured", provider.as_str()))
                }
                VerifyError::KeysUnavailable(reason) => SignInError::Unavailable(reason),
            })?;
        self.claim_nonce(&verified).await?;
        let identity = VerifiedIdentity {
            provider,
            subject: verified.subject.clone(),
            email: verified.email.clone(),
        };
        Ok(self.identity.find_account(&identity).await?)
    }

    /// Revokes the account's stored Apple refresh tokens at Apple (App Store rule for account
    /// deletion, T-152) and clears them. Best effort per token: a failure is logged and the token
    /// is cleared anyway, so erasure never waits on Apple. Returns how many were revoked.
    pub async fn revoke_apple_tokens(&self, account_id: Uuid) -> Result<usize, SignInError> {
        let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT subject, apple_refresh_token_enc FROM identity.auth_identities
             WHERE account_id = $1 AND provider = 'apple' AND apple_refresh_token_enc IS NOT NULL",
        )
        .bind(account_id)
        .fetch_all(self.identity.pool())
        .await
        .map_err(|e| SignInError::Store(StoreError::Database(e)))?;
        let client_id = self
            .verifiers
            .get(&Provider::Apple)
            .and_then(|v| v.client_ids().first().cloned());
        let mut revoked = 0;
        for (subject, encrypted) in rows {
            let token = self
                .cipher
                .decrypt(&encrypted, &apple_refresh_token_aad(&subject))
                .ok()
                .and_then(|plain| String::from_utf8(plain).ok());
            match (token, client_id.as_deref()) {
                (Some(token), Some(client_id)) => {
                    match self
                        .apple_tokens
                        .revoke_refresh_token(client_id, &token)
                        .await
                    {
                        Ok(()) => revoked += 1,
                        Err(err) => {
                            tracing::warn!(error = %err, account_id = %account_id, "Apple token revocation failed")
                        }
                    }
                }
                _ => {
                    tracing::warn!(account_id = %account_id, "Apple token not revoked (unreadable or Apple not configured)")
                }
            }
            sqlx::query(
                "UPDATE identity.auth_identities SET apple_refresh_token_enc = NULL
                 WHERE provider = 'apple' AND subject = $1",
            )
            .bind(&subject)
            .execute(self.identity.pool())
            .await
            .map_err(|e| SignInError::Store(StoreError::Database(e)))?;
        }
        Ok(revoked)
    }

    pub async fn sign_in(
        &self,
        provider: Provider,
        request: SignInRequest,
    ) -> Result<SignedIn, SignInError> {
        let verifier = self.verifiers.get(&provider).ok_or_else(|| {
            SignInError::Unavailable(format!("{} not enabled", provider.as_str()))
        })?;
        let verified = verifier
            .verify(&request.id_token, &request.raw_nonce)
            .await
            .map_err(|err| match err {
                VerifyError::Invalid(reason) => SignInError::Rejected(reason),
                VerifyError::NotConfigured => {
                    SignInError::Unavailable(format!("{} not configured", provider.as_str()))
                }
                VerifyError::KeysUnavailable(reason) => SignInError::Unavailable(reason),
            })?;
        self.claim_nonce(&verified).await?;

        let identity = VerifiedIdentity {
            provider,
            subject: verified.subject.clone(),
            email: verified.email.clone(),
        };
        let (account_id, created) = self
            .identity
            .find_or_create_account(&identity, &request.signals)
            .await?;
        let status = self.identity.account_status(account_id).await?;
        if matches!(status, AccountStatus::Suspended | AccountStatus::Banned) {
            return Err(SignInError::Blocked(status));
        }

        if provider == Provider::Apple
            && let Some(code) = request.authorization_code.as_deref()
        {
            self.store_apple_refresh_token(verifier, &verified, account_id, code)
                .await;
        }

        let session = self
            .identity
            .start_session(account_id, request.device.as_ref())
            .await?;
        let entry = AuditEntry::new(Actor::User(account_id), AuditAction::SignIn)
            .target("session", session.session_id)
            .detail("provider", provider.as_str())
            .detail("new_account", created);
        audit::record_or_log(self.identity.pool(), &entry, self.identity.clock.now()).await;
        tracing::info!(
            event = "auth.sign_in",
            provider = provider.as_str(),
            account_id = %account_id,
            created,
            "signed in"
        );
        Ok(SignedIn { session, created })
    }

    /// Records the nonce as used (Redis `SET NX` with a TTL covering the token's remaining
    /// lifetime plus skew). A second accepted token with the same nonce is a replay.
    async fn claim_nonce(&self, verified: &VerifiedIdToken) -> Result<(), SignInError> {
        let nonce_hash = verified
            .nonce_hash
            .as_deref()
            .ok_or(SignInError::Rejected("nonce missing"))?;
        let mut redis = self
            .identity
            .cache
            .redis()
            .ok_or_else(|| SignInError::Unavailable("no Redis configured".to_owned()))?;
        let remaining = (verified.expires_at - self.identity.clock.now()).whole_seconds();
        // + 5 min covers the clock skew the verifier tolerates (at most 300 s by config).
        let ttl = (remaining + 300).clamp(1, MAX_NONCE_TTL_SECS);
        let ttl = u64::try_from(ttl).unwrap_or(1);
        let key = format!("auth:nonce:{}:{nonce_hash}", verified.provider.as_str());
        let options = SetOptions::default()
            .conditional_set(redis::ExistenceCheck::NX)
            .with_expiration(SetExpiry::EX(ttl));
        let stored: Option<String> = redis
            .set_options(key, 1, options)
            .await
            .map_err(|err| SignInError::Unavailable(format!("nonce store: {err}")))?;
        if stored.is_none() {
            tracing::warn!(
                event = "auth.nonce_replay",
                provider = verified.provider.as_str(),
                "ID token nonce replayed"
            );
            return Err(SignInError::Replayed);
        }
        Ok(())
    }

    /// Best effort: sign-in does not fail when Apple's token endpoint does. A missing refresh
    /// token only means account deletion cannot revoke the Apple grant (logged).
    async fn store_apple_refresh_token(
        &self,
        verifier: &IdTokenVerifier,
        verified: &VerifiedIdToken,
        account_id: Uuid,
        code: &str,
    ) {
        let tokens = match self
            .apple_tokens
            .exchange_code(&verified.client_id, code)
            .await
        {
            Ok(tokens) => tokens,
            Err(AppleTokenError::NotConfigured) => {
                tracing::debug!("Apple client secret not configured; code exchange skipped");
                return;
            }
            Err(err) => {
                tracing::warn!(error = %err, account_id = %account_id, "Apple code exchange failed");
                return;
            }
        };
        // The code must belong to the same Apple user as the ID token.
        match verifier.verify_token(&tokens.id_token).await {
            Ok(exchanged) if exchanged.subject == verified.subject => {}
            Ok(_) => {
                tracing::warn!(account_id = %account_id, "Apple code belongs to another user; ignored");
                return;
            }
            Err(err) => {
                tracing::warn!(error = %err, account_id = %account_id, "Apple token response did not verify");
                return;
            }
        }
        let encrypted = match self.cipher.encrypt(
            tokens.refresh_token.as_bytes(),
            &apple_refresh_token_aad(&verified.subject),
        ) {
            Ok(encrypted) => encrypted,
            Err(err) => {
                tracing::error!(error = %err, "could not encrypt the Apple refresh token");
                return;
            }
        };
        if let Err(err) = self
            .identity
            .store_apple_refresh_token(&verified.subject, &encrypted)
            .await
        {
            tracing::error!(error = %err, account_id = %account_id, "could not store the Apple refresh token");
        }
    }
}

/// Associated data binding an encrypted Apple refresh token to its identity row.
pub fn apple_refresh_token_aad(subject: &str) -> Vec<u8> {
    format!("identity.auth_identities.apple_refresh_token_enc:apple:{subject}").into_bytes()
}

impl IdentityService {
    pub(crate) async fn store_apple_refresh_token(
        &self,
        subject: &str,
        encrypted: &[u8],
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE identity.auth_identities SET apple_refresh_token_enc = $2
             WHERE provider = 'apple' AND subject = $1",
        )
        .bind(subject)
        .bind(encrypted)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The stored, still-encrypted Apple refresh token of an Apple identity.
    pub async fn apple_refresh_token_enc(
        &self,
        subject: &str,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let row: Option<Option<Vec<u8>>> = sqlx::query_scalar(
            "SELECT apple_refresh_token_enc FROM identity.auth_identities
             WHERE provider = 'apple' AND subject = $1",
        )
        .bind(subject)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.flatten())
    }
}
