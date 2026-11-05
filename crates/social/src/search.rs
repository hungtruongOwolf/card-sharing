//! Exact-handle search without enumeration (US-5a, SR-19, SR-23, threat A-11).
//!
//! One code path for every outcome: the handle lookup, the age check, the block check and the
//! profile read run whether or not the handle exists, and the HTTP layer pads every answer to a
//! fixed latency bucket. A hidden account (under `discovery.min_age`, not active, blocked either
//! way, or the caller) answers exactly like a missing handle.
//!
//! A hit carries a `ref` instead of an id: the target id and an expiry, encrypted with the field
//! cipher and bound to the caller (associated data), so it cannot be read, forged, extended or
//! used by another account. A friend request from a ref re-checks visibility and is silently
//! dropped when the target became hidden.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use identity::AgeError;
use policy::PolicyContext;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::error::SocialError;
use crate::graph::blocked_either_way;
use crate::service::SocialService;

/// A visible account found by its exact handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Opaque; valid for `search.ref_ttl_secs` and only for the account that searched.
    pub search_ref: String,
    pub display_name: String,
}

/// Lowercases (ASCII) and checks the handle shape; anything else can never match. A single
/// leading `@` is accepted. Returns `None` for input that cannot be a handle.
fn normalize(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let raw = raw.strip_prefix('@').unwrap_or(raw);
    if !(3..=20).contains(&raw.len()) {
        return None;
    }
    let handle = raw.to_ascii_lowercase();
    handle
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_'))
        .then_some(handle)
}

fn ref_aad(caller: Uuid) -> Vec<u8> {
    let mut aad = b"social.search_ref:".to_vec();
    aad.extend_from_slice(caller.as_bytes());
    aad
}

impl SocialService {
    /// Exact, case-insensitive handle search on behalf of `caller`.
    pub async fn search_by_handle(
        &self,
        caller: Uuid,
        raw_handle: &str,
    ) -> Result<Option<SearchHit>, SocialError> {
        // An impossible handle keeps the lookup (and everything after it) on the same path.
        let handle = normalize(raw_handle).unwrap_or_default();
        let found = self
            .identity
            .handle_owner(&handle)
            .await
            .map_err(|err| SocialError::Identity(err.to_string()))?;
        let target = found.unwrap_or(Uuid::nil());
        let visible = self.discoverable(caller, target).await?;
        let profiles = self.summaries(&[target]).await?;
        let (Some(_), true, Some(profile)) = (found, visible, profiles.get(&target)) else {
            return Ok(None);
        };
        Ok(Some(SearchHit {
            search_ref: self.mint_ref(caller, target)?,
            display_name: profile.display_name.clone(),
        }))
    }

    /// May `caller` find (and silently request) `target`: old enough for discovery in the
    /// target's region, not the caller, no block either way. Runs the same queries for any id.
    pub(crate) async fn discoverable(
        &self,
        caller: Uuid,
        target: Uuid,
    ) -> Result<bool, SocialError> {
        let age_ok = match self.age.status(target, &PolicyContext::default()).await {
            Ok(status) => status.discoverable,
            Err(AgeError::NotFound) => false,
            Err(err) => return Err(SocialError::Identity(err.to_string())),
        };
        let mut conn = self.pool.acquire().await?;
        let blocked = blocked_either_way(&mut conn, caller, target).await?;
        Ok(age_ok && !blocked && target != caller)
    }

    /// Visibility re-checked when a request is sent from a ref: discoverable and still active.
    pub(crate) async fn target_still_findable(
        &self,
        caller: Uuid,
        target: Uuid,
    ) -> Result<bool, SocialError> {
        let visible = self.discoverable(caller, target).await?;
        let active = self.summaries(&[target]).await?.contains_key(&target);
        Ok(visible && active)
    }

    fn mint_ref(&self, caller: Uuid, target: Uuid) -> Result<String, SocialError> {
        let expires =
            self.clock.now() + Duration::seconds(i64::from(self.settings.search_ref_ttl_secs));
        let mut plain = target.as_bytes().to_vec();
        plain.extend_from_slice(&expires.unix_timestamp().to_be_bytes());
        let sealed = self.cipher.encrypt(&plain, &ref_aad(caller))?;
        Ok(URL_SAFE_NO_PAD.encode(sealed))
    }

    /// The target of a ref minted for `caller` and not yet expired.
    pub(crate) fn open_ref(&self, caller: Uuid, search_ref: &str) -> Option<Uuid> {
        if search_ref.len() > 256 {
            return None;
        }
        let sealed = URL_SAFE_NO_PAD.decode(search_ref).ok()?;
        let plain = self.cipher.decrypt(&sealed, &ref_aad(caller)).ok()?;
        let (id, expires) = plain.split_at_checked(16)?;
        let target = Uuid::from_slice(id).ok()?;
        let expires = i64::from_be_bytes(expires.try_into().ok()?);
        let expires = OffsetDateTime::from_unix_timestamp(expires).ok()?;
        (self.clock.now() < expires).then_some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_handle_shapes_are_searched() {
        assert_eq!(normalize("Mai.Cat"), Some("mai.cat".to_owned()));
        assert_eq!(normalize(" @mai_cat "), Some("mai_cat".to_owned()));
        assert_eq!(normalize("ma"), None);
        assert_eq!(normalize("mai cat"), None);
        assert_eq!(normalize("mai%"), None);
        assert_eq!(normalize("mai*"), None);
        assert_eq!(normalize("@@mai"), None);
        assert_eq!(normalize(&"a".repeat(21)), None);
        assert_eq!(normalize("m\u{e0}i"), None);
    }

    #[test]
    fn refs_are_bound_to_the_caller() {
        assert_ne!(ref_aad(Uuid::now_v7()), ref_aad(Uuid::now_v7()));
    }
}
