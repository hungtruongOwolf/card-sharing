//! Region / jurisdiction policy.
//!
//! Rules (min age, AI routing, ads, AI labelling) come from a versioned YAML config
//! (`config/policy/regions.yaml`), never from `if country == ...` in code.
//! Consumers depend on [`PolicyService::resolve`] and never on raw country codes.
//!
//! Resolution per signal: default → region-group override (e.g. `EU`) → country override.
//! When several signals are present (store country, IP country, declared country) and
//! disagree, the **strictest** value of every field wins.
//! See `context/wiki/architecture/region-policy.md`.

mod config;
mod country;

use std::path::Path;

pub use config::PolicyConfigError;
pub use country::{CountryCode, InvalidCountryCode};
use serde::{Deserialize, Serialize};

/// Fully resolved policy for one request/user. Every field has a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub min_age: u8,
    /// Age of majority: below it an account is a minor (minor protections apply).
    #[serde(default = "default_adult_age")]
    pub adult_age: u8,
    #[serde(default)]
    pub discovery: DiscoveryPolicy,
    #[serde(default)]
    pub invites: InvitePolicy,
    #[serde(default)]
    pub public_links: PublicLinksPolicy,
    pub ai_effects: AiEffectsPolicy,
    pub ads: AdsPolicy,
    pub ai_label: AiLabelPolicy,
}

/// Handle search (US-5a): accounts younger than `min_age` are never returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryPolicy {
    pub min_age: u8,
}

impl Default for DiscoveryPolicy {
    fn default() -> Self {
        Self { min_age: 18 }
    }
}

/// Personal invite links of minors (US-4 AC-4.4, SR-20): they expire and have a use cap. Adults'
/// invites never expire on their own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvitePolicy {
    /// Days a minor's invite stays valid after it was issued.
    pub minor_ttl_days: u16,
    /// Friend requests a minor's invite can create before it expires.
    pub minor_max_uses: u32,
}

impl Default for InvitePolicy {
    fn default() -> Self {
        Self {
            minor_ttl_days: 7,
            minor_max_uses: 5,
        }
    }
}

/// Public board links (US-24 AC-24.6): owners younger than `min_age` cannot create them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicLinksPolicy {
    pub min_age: u8,
}

impl Default for PublicLinksPolicy {
    fn default() -> Self {
        Self { min_age: 18 }
    }
}

fn default_adult_age() -> u8 {
    18
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiEffectsPolicy {
    /// Whether cloud (third-party) AI effects may be offered at all.
    pub cloud_allowed: bool,
    /// Where photos that contain people may be processed.
    pub people_in_photo: PeopleInPhotoRouting,
}

/// Ordered from least to most strict; `Ord` is used to pick the strictest value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeopleInPhotoRouting {
    Any,
    OnDeviceOrNonGemini,
    OnDeviceOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdsPolicy {
    pub rewarded: bool,
    pub personalized_requires_consent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiLabelPolicy {
    pub visible_badge: bool,
    pub c2pa: bool,
}

/// Jurisdiction signals known for the caller. All optional; none means "default".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolicyContext {
    pub store_country: Option<CountryCode>,
    pub ip_country: Option<CountryCode>,
    pub declared_country: Option<CountryCode>,
}

impl PolicyContext {
    fn signals(&self) -> impl Iterator<Item = CountryCode> + '_ {
        [self.store_country, self.ip_country, self.declared_country]
            .into_iter()
            .flatten()
    }
}

/// Resolves [`Policy`] values from a validated region config.
#[derive(Debug, Clone)]
pub struct PolicyService {
    config: config::RegionConfig,
}

impl PolicyService {
    /// Parses and validates a region config. Fails on unknown keys, invalid country codes,
    /// overrides that reference unknown groups, and countries listed in more than one group.
    pub fn from_yaml_str(yaml: &str) -> Result<Self, PolicyConfigError> {
        Ok(Self {
            config: config::RegionConfig::parse(yaml)?,
        })
    }

    /// Reads and validates a region config file.
    pub fn from_path(path: &Path) -> Result<Self, PolicyConfigError> {
        let yaml = std::fs::read_to_string(path).map_err(|source| PolicyConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_yaml_str(&yaml)
    }

    /// Config schema version (`version:` in the YAML), useful for logs and readiness output.
    pub fn config_version(&self) -> u32 {
        self.config.version
    }

    /// Returns the policy for the given context: strictest across all present signals,
    /// or the default policy when no signal is present.
    pub fn resolve(&self, ctx: &PolicyContext) -> Policy {
        ctx.signals()
            .map(|country| self.config.resolve_country(country))
            .reduce(strictest)
            .unwrap_or_else(|| self.config.default.clone())
    }
}

impl PolicyService {
    /// Strictest policy across several contexts, e.g. the signals stored at sign-up and those
    /// of the current request (SR-24: a later request from a laxer region never loosens rules).
    pub fn resolve_strictest(&self, contexts: &[PolicyContext]) -> Policy {
        contexts
            .iter()
            .map(|ctx| self.resolve(ctx))
            .reduce(strictest)
            .unwrap_or_else(|| self.config.default.clone())
    }
}

/// Field-wise strictest combination of two policies.
fn strictest(a: Policy, b: Policy) -> Policy {
    Policy {
        min_age: a.min_age.max(b.min_age),
        adult_age: a.adult_age.max(b.adult_age),
        discovery: DiscoveryPolicy {
            min_age: a.discovery.min_age.max(b.discovery.min_age),
        },
        invites: InvitePolicy {
            minor_ttl_days: a.invites.minor_ttl_days.min(b.invites.minor_ttl_days),
            minor_max_uses: a.invites.minor_max_uses.min(b.invites.minor_max_uses),
        },
        public_links: PublicLinksPolicy {
            min_age: a.public_links.min_age.max(b.public_links.min_age),
        },
        ai_effects: AiEffectsPolicy {
            cloud_allowed: a.ai_effects.cloud_allowed && b.ai_effects.cloud_allowed,
            people_in_photo: a
                .ai_effects
                .people_in_photo
                .max(b.ai_effects.people_in_photo),
        },
        ads: AdsPolicy {
            rewarded: a.ads.rewarded && b.ads.rewarded,
            personalized_requires_consent: a.ads.personalized_requires_consent
                || b.ads.personalized_requires_consent,
        },
        ai_label: AiLabelPolicy {
            visible_badge: a.ai_label.visible_badge || b.ai_label.visible_badge,
            c2pa: a.ai_label.c2pa || b.ai_label.c2pa,
        },
    }
}
