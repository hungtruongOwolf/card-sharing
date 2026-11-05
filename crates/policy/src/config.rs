//! YAML schema for `config/policy/regions.yaml` and its validation.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

use crate::country::CountryCode;
use crate::{AdsPolicy, AiEffectsPolicy, AiLabelPolicy, PeopleInPhotoRouting, Policy};

const SUPPORTED_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum PolicyConfigError {
    #[error("cannot read policy config {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid policy config YAML: {0}")]
    Yaml(#[from] serde_norway::Error),
    #[error("unsupported policy config version {found} (supported: {SUPPORTED_VERSION})")]
    UnsupportedVersion { found: u32 },
    #[error("group {group:?}: {reason}")]
    InvalidGroup { group: String, reason: String },
    #[error(
        "override {key:?}: not a defined group and not a valid ISO 3166-1 alpha-2 country code"
    )]
    UnknownOverrideKey { key: String },
}

/// Raw file shape. Unknown keys are rejected so typos fail fast instead of silently
/// falling back to defaults.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    default: Policy,
    #[serde(default)]
    groups: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    overrides: BTreeMap<String, PolicyOverride>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyOverride {
    min_age: Option<u8>,
    adult_age: Option<u8>,
    discovery: Option<DiscoveryOverride>,
    invites: Option<InviteOverride>,
    ai_effects: Option<AiEffectsOverride>,
    ads: Option<AdsOverride>,
    ai_label: Option<AiLabelOverride>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscoveryOverride {
    min_age: Option<u8>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct InviteOverride {
    minor_ttl_days: Option<u16>,
    minor_max_uses: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AiEffectsOverride {
    cloud_allowed: Option<bool>,
    people_in_photo: Option<PeopleInPhotoRouting>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdsOverride {
    rewarded: Option<bool>,
    personalized_requires_consent: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AiLabelOverride {
    visible_badge: Option<bool>,
    c2pa: Option<bool>,
}

/// Validated, lookup-friendly config.
#[derive(Debug, Clone)]
pub(crate) struct RegionConfig {
    pub(crate) version: u32,
    pub(crate) default: Policy,
    /// country → group override (at most one group per country).
    group_override_by_country: HashMap<CountryCode, PolicyOverride>,
    country_overrides: HashMap<CountryCode, PolicyOverride>,
}

impl RegionConfig {
    pub(crate) fn parse(yaml: &str) -> Result<Self, PolicyConfigError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(PolicyConfigError::UnsupportedVersion { found: raw.version });
        }

        let group_of = index_group_members(&raw.groups)?;

        let mut group_overrides: HashMap<&str, PolicyOverride> = HashMap::new();
        let mut country_overrides = HashMap::new();
        for (key, value) in raw.overrides {
            if let Some((name, _)) = raw.groups.get_key_value(&key) {
                group_overrides.insert(name.as_str(), value);
            } else {
                let country = key
                    .parse::<CountryCode>()
                    .map_err(|_| PolicyConfigError::UnknownOverrideKey { key: key.clone() })?;
                country_overrides.insert(country, value);
            }
        }

        let group_override_by_country = group_of
            .into_iter()
            .filter_map(|(country, group)| {
                group_overrides
                    .get(group)
                    .map(|value| (country, value.clone()))
            })
            .collect();

        Ok(Self {
            version: raw.version,
            default: raw.default,
            group_override_by_country,
            country_overrides,
        })
    }

    /// default → group override → country override (most specific wins).
    pub(crate) fn resolve_country(&self, country: CountryCode) -> Policy {
        let mut policy = self.default.clone();
        if let Some(group) = self.group_override_by_country.get(&country) {
            apply(&mut policy, group);
        }
        if let Some(own) = self.country_overrides.get(&country) {
            apply(&mut policy, own);
        }
        policy
    }
}

/// Maps each member country to its group name; rejects invalid codes and
/// countries listed in more than one group (ambiguous precedence).
fn index_group_members(
    groups: &BTreeMap<String, Vec<String>>,
) -> Result<HashMap<CountryCode, &str>, PolicyConfigError> {
    let mut group_of: HashMap<CountryCode, &str> = HashMap::new();
    for (group, members) in groups {
        if members.is_empty() {
            return Err(invalid_group(group, "group has no members".to_owned()));
        }
        for member in members {
            let country = member
                .parse::<CountryCode>()
                .map_err(|err| invalid_group(group, err.to_string()))?;
            if let Some(previous) = group_of.insert(country, group.as_str()) {
                return Err(invalid_group(
                    group,
                    format!("{country} is already a member of group {previous:?}"),
                ));
            }
        }
    }
    Ok(group_of)
}

fn invalid_group(group: &str, reason: String) -> PolicyConfigError {
    PolicyConfigError::InvalidGroup {
        group: group.to_owned(),
        reason,
    }
}

fn apply(policy: &mut Policy, o: &PolicyOverride) {
    if let Some(min_age) = o.min_age {
        policy.min_age = min_age;
    }
    if let Some(adult_age) = o.adult_age {
        policy.adult_age = adult_age;
    }
    if let Some(min_age) = o.discovery.as_ref().and_then(|d| d.min_age) {
        policy.discovery.min_age = min_age;
    }
    if let Some(invites) = &o.invites {
        if let Some(days) = invites.minor_ttl_days {
            policy.invites.minor_ttl_days = days;
        }
        if let Some(uses) = invites.minor_max_uses {
            policy.invites.minor_max_uses = uses;
        }
    }
    if let Some(ai) = &o.ai_effects {
        apply_ai_effects(&mut policy.ai_effects, ai);
    }
    if let Some(ads) = &o.ads {
        apply_ads(&mut policy.ads, ads);
    }
    if let Some(label) = &o.ai_label {
        apply_ai_label(&mut policy.ai_label, label);
    }
}

fn apply_ai_effects(target: &mut AiEffectsPolicy, o: &AiEffectsOverride) {
    if let Some(v) = o.cloud_allowed {
        target.cloud_allowed = v;
    }
    if let Some(v) = o.people_in_photo {
        target.people_in_photo = v;
    }
}

fn apply_ads(target: &mut AdsPolicy, o: &AdsOverride) {
    if let Some(v) = o.rewarded {
        target.rewarded = v;
    }
    if let Some(v) = o.personalized_requires_consent {
        target.personalized_requires_consent = v;
    }
}

fn apply_ai_label(target: &mut AiLabelPolicy, o: &AiLabelOverride) {
    if let Some(v) = o.visible_badge {
        target.visible_badge = v;
    }
    if let Some(v) = o.c2pa {
        target.c2pa = v;
    }
}
