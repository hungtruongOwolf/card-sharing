use std::fmt;
use std::str::FromStr;

use policy::PolicyContext;
use serde::{Deserialize, Serialize};

/// Lifecycle of an account (`identity.accounts.status`, see data-model.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    /// Signed in, date of birth not submitted yet.
    PendingAge,
    /// Age gate passed, profile (display name, handle) not finished yet.
    PendingProfile,
    Active,
    Suspended,
    Banned,
    DeletionPending,
}

impl AccountStatus {
    pub const ALL: [Self; 6] = [
        Self::PendingAge,
        Self::PendingProfile,
        Self::Active,
        Self::Suspended,
        Self::Banned,
        Self::DeletionPending,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PendingAge => "pending_age",
            Self::PendingProfile => "pending_profile",
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Banned => "banned",
            Self::DeletionPending => "deletion_pending",
        }
    }

    /// Still in the onboarding flow (only `onb` routes are open).
    pub fn is_onboarding(self) -> bool {
        matches!(self, Self::PendingAge | Self::PendingProfile)
    }
}

/// What a session token may be used for (`identity.sessions.scope`, ADR-0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionScope {
    /// Signed in, onboarding not finished.
    Onboarding,
    /// Full user session.
    Full,
    /// Read-only widget token, a child of a full device session (T-91): `wid` routes only.
    Widget,
}

impl SessionScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Onboarding => "onboarding",
            Self::Full => "full",
            Self::Widget => "widget",
        }
    }
}

/// Jurisdiction signals stored with the account (`identity.accounts.region_signals`, SR-24):
/// ISO 3166-1 alpha-2 codes, each optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionSignals {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared: Option<String>,
}

impl RegionSignals {
    /// As a policy context; invalid stored codes are ignored.
    pub fn context(&self) -> PolicyContext {
        let parse = |value: &Option<String>| value.as_deref().and_then(|v| v.parse().ok());
        PolicyContext {
            store_country: parse(&self.store),
            ip_country: parse(&self.ip),
            declared_country: parse(&self.declared),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown value {0:?}")]
pub struct UnknownValue(pub String);

impl FromStr for AccountStatus {
    type Err = UnknownValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
            .ok_or_else(|| UnknownValue(value.to_owned()))
    }
}

impl FromStr for SessionScope {
    type Err = UnknownValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "onboarding" => Ok(Self::Onboarding),
            "full" => Ok(Self::Full),
            "widget" => Ok(Self::Widget),
            other => Err(UnknownValue(other.to_owned())),
        }
    }
}

impl fmt::Display for AccountStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trips_through_its_database_value() {
        for status in AccountStatus::ALL {
            assert_eq!(status.as_str().parse::<AccountStatus>(), Ok(status));
        }
        assert!("root".parse::<AccountStatus>().is_err());
    }

    #[test]
    fn region_signals_become_a_policy_context() {
        let signals: RegionSignals =
            serde_json::from_str(r#"{"store":"US","ip":"de","declared":"??"}"#).unwrap();
        let ctx = signals.context();
        assert_eq!(ctx.store_country.map(|c| c.to_string()), Some("US".into()));
        assert_eq!(ctx.ip_country.map(|c| c.to_string()), Some("DE".into()));
        assert!(ctx.declared_country.is_none(), "invalid codes are ignored");
    }

    #[test]
    fn scope_round_trips() {
        for scope in [
            SessionScope::Onboarding,
            SessionScope::Full,
            SessionScope::Widget,
        ] {
            assert_eq!(scope.as_str().parse::<SessionScope>(), Ok(scope));
        }
    }
}
