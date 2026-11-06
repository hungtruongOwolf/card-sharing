//! Identity's personal data (T-150): account, date of birth, profile, sign-in methods, devices
//! and sessions. Erasing deletes the account row last (registered first in the registry), which
//! also cascades every foreign key left in other schemas.

use std::sync::Arc;

use platform::crypto::FieldCipher;
use platform::personal_data::{
    ExportSection, PersonalData, PersonalDataError, PersonalDataFuture, query_json,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::service::IdentityService;

pub struct IdentityPersonalData {
    pub identity: Arc<IdentityService>,
    pub cipher: Arc<FieldCipher>,
}

const ACCOUNT: &str = "SELECT jsonb_build_object('id', a.id, 'status', a.status,
    'created_at', a.created_at, 'region_signals', a.region_signals,
    'deletion_requested_at', a.deletion_requested_at,
    'os_age_signal', a.os_age_signal, 'os_age_signal_at', a.os_age_signal_at)
  FROM identity.accounts a WHERE a.id = $1";
const PROFILE: &str =
    "SELECT jsonb_build_object('display_name', p.display_name, 'handle', p.handle,
    'handle_claimed_at', p.handle_claimed_at, 'findable_by_handle', p.findable_by_handle,
    'created_at', p.created_at)
  FROM identity.profiles p WHERE p.account_id = $1";
const SIGN_IN: &str =
    "SELECT jsonb_agg(jsonb_build_object('provider', i.provider, 'email', i.email,
    'created_at', i.created_at) ORDER BY i.created_at)
  FROM identity.auth_identities i WHERE i.account_id = $1";
const DEVICES: &str = "SELECT jsonb_agg(jsonb_build_object('id', d.id, 'platform', d.platform,
    'app_version', d.app_version, 'created_at', d.created_at, 'last_seen_at', d.last_seen_at)
    ORDER BY d.created_at)
  FROM identity.devices d WHERE d.account_id = $1";
const SESSIONS: &str = "SELECT jsonb_agg(jsonb_build_object('id', s.id, 'scope', s.scope,
    'device_id', s.device_id, 'client_label', s.client_label, 'ip_country', s.ip_country,
    'created_at', s.created_at, 'last_seen_at', s.last_seen_at, 'revoked_at', s.revoked_at,
    'revoked_reason', s.revoked_reason) ORDER BY s.created_at)
  FROM identity.sessions s WHERE s.account_id = $1";

impl IdentityPersonalData {
    async fn birth_date(&self, account: Uuid) -> Result<Option<String>, PersonalDataError> {
        let stored: Option<Option<Vec<u8>>> =
            sqlx::query_scalar("SELECT birth_date_enc FROM identity.accounts WHERE id = $1")
                .bind(account)
                .fetch_optional(self.identity.pool())
                .await?;
        let Some(Some(stored)) = stored else {
            return Ok(None);
        };
        let plain = self
            .cipher
            .decrypt(&stored, account.as_bytes())
            .map_err(|e| PersonalDataError::Other(format!("birth date: {e}")))?;
        Ok(Some(String::from_utf8_lossy(&plain).into_owned()))
    }
}

impl PersonalData for IdentityPersonalData {
    fn module(&self) -> &'static str {
        "identity"
    }

    fn tables(&self) -> &'static [&'static str] {
        &[
            "identity.accounts",
            "identity.auth_identities",
            "identity.devices",
            "identity.sessions",
            "identity.refresh_tokens",
            "identity.profiles",
            "identity.handle_reservations",
            "identity.attestation_keys",
        ]
    }

    fn export(&self, account: Uuid) -> PersonalDataFuture<'_, ExportSection> {
        Box::pin(async move {
            let pool = self.identity.pool();
            let mut account_json = query_json(pool, ACCOUNT, account).await?;
            if let Value::Object(map) = &mut account_json {
                map.insert(
                    "birth_date".to_owned(),
                    json!(self.birth_date(account).await?),
                );
            }
            Ok(ExportSection {
                data: json!({
                    "account": account_json,
                    "profile": query_json(pool, PROFILE, account).await?,
                    "sign_in_methods": query_json(pool, SIGN_IN, account).await?,
                    "devices": query_json(pool, DEVICES, account).await?,
                    "sessions": query_json(pool, SESSIONS, account).await?,
                }),
                files: Vec::new(),
            })
        })
    }

    fn hide(&self, _account: Uuid) -> PersonalDataFuture<'_, ()> {
        // The account status (`deletion_pending`) and session revocation are the deletion flow's
        // job (T-151); every read predicate already requires an active account.
        Box::pin(async { Ok(()) })
    }

    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            let pool = self.identity.pool();
            let mut tx = pool.begin().await?;
            sqlx::query("DELETE FROM identity.handle_reservations WHERE released_by = $1")
                .bind(account)
                .execute(&mut *tx)
                .await?;
            // Cascades auth identities, devices, sessions (and refresh tokens), profile and
            // attestation keys.
            sqlx::query("DELETE FROM identity.accounts WHERE id = $1")
                .bind(account)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            if let Err(err) = self.identity.invalidate_cached_sessions(account).await {
                tracing::warn!(error = %err, "cached sessions not dropped after erase");
            }
            Ok(())
        })
    }
}
