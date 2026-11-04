// Test helpers may unwrap: a failure here is a test failure, not a request-path panic.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use policy::{
    CountryCode, PeopleInPhotoRouting, Policy, PolicyConfigError, PolicyContext, PolicyService,
};

/// The real, shipped config must always parse — a broken file must fail CI, not production.
const SHIPPED_CONFIG: &str = include_str!("../../../config/policy/regions.yaml");

const FIXTURE: &str = r#"
version: 1
default:
  min_age: 13
  ai_effects: { cloud_allowed: true, people_in_photo: on_device_or_non_gemini }
  ads: { rewarded: true, personalized_requires_consent: false }
  ai_label: { visible_badge: true, c2pa: true }
groups:
  EU: [DE, FR, IE]
overrides:
  EU: { min_age: 16, ads: { personalized_requires_consent: true } }
  IE: { min_age: 15 }
  KR: { min_age: 14, ai_effects: { cloud_allowed: false, people_in_photo: on_device_only } }
"#;

fn service() -> PolicyService {
    PolicyService::from_yaml_str(FIXTURE).unwrap()
}

fn cc(code: &str) -> CountryCode {
    code.parse().unwrap()
}

fn ctx_store(code: &str) -> PolicyContext {
    PolicyContext {
        store_country: Some(cc(code)),
        ..PolicyContext::default()
    }
}

fn default_policy(svc: &PolicyService) -> Policy {
    svc.resolve(&PolicyContext::default())
}

#[test]
fn shipped_config_is_valid() {
    let svc = PolicyService::from_yaml_str(SHIPPED_CONFIG).unwrap();
    assert_eq!(svc.config_version(), 1);
}

#[test]
fn policy_resolves_eu_min_age_16() {
    let svc = PolicyService::from_yaml_str(SHIPPED_CONFIG).unwrap();
    let policy = svc.resolve(&ctx_store("DE"));
    assert_eq!(policy.min_age, 16);
    assert!(policy.ads.personalized_requires_consent);
}

#[test]
fn shipped_config_resolves_kr_min_age_14_and_unknown_to_default() {
    let svc = PolicyService::from_yaml_str(SHIPPED_CONFIG).unwrap();
    assert_eq!(svc.resolve(&ctx_store("KR")).min_age, 14);
    assert_eq!(svc.resolve(&ctx_store("BR")), default_policy(&svc));
}

#[test]
fn no_signal_resolves_to_default() {
    let svc = service();
    let policy = default_policy(&svc);
    assert_eq!(policy.min_age, 13);
    assert!(!policy.ads.personalized_requires_consent);
}

#[test]
fn group_override_applies_to_members_and_keeps_other_defaults() {
    let policy = service().resolve(&ctx_store("FR"));
    assert_eq!(policy.min_age, 16);
    assert!(policy.ads.personalized_requires_consent);
    assert!(
        policy.ads.rewarded,
        "fields not overridden keep the default"
    );
}

#[test]
fn country_override_beats_group_override() {
    let policy = service().resolve(&ctx_store("IE"));
    assert_eq!(policy.min_age, 15, "country override wins over EU group");
    assert!(
        policy.ads.personalized_requires_consent,
        "group fields still apply"
    );
}

#[test]
fn conflicting_signals_take_the_strictest_value_per_field() {
    let svc = service();
    // FR (EU: min_age 16, consent) vs KR (min_age 14, no cloud AI, on-device only).
    let policy = svc.resolve(&PolicyContext {
        store_country: Some(cc("FR")),
        ip_country: Some(cc("KR")),
        declared_country: Some(cc("BR")),
    });
    assert_eq!(policy.min_age, 16);
    assert!(policy.ads.personalized_requires_consent);
    assert!(!policy.ai_effects.cloud_allowed);
    assert_eq!(
        policy.ai_effects.people_in_photo,
        PeopleInPhotoRouting::OnDeviceOnly
    );
}

#[test]
fn country_codes_are_case_insensitive() {
    assert_eq!(service().resolve(&ctx_store("de")).min_age, 16);
}

#[test]
fn rejects_unknown_fields() {
    let yaml = FIXTURE.replace("min_age: 13", "min_age: 13\n  min_agee: 99");
    let err = PolicyService::from_yaml_str(&yaml).unwrap_err();
    assert!(matches!(err, PolicyConfigError::Yaml(_)), "{err}");
}

#[test]
fn rejects_override_key_that_is_neither_group_nor_country() {
    let yaml = FIXTURE.replace("KR:", "KOREA:");
    let err = PolicyService::from_yaml_str(&yaml).unwrap_err();
    assert!(
        matches!(err, PolicyConfigError::UnknownOverrideKey { ref key } if key == "KOREA"),
        "{err}"
    );
}

#[test]
fn rejects_country_in_two_groups() {
    let yaml = FIXTURE.replace("EU: [DE, FR, IE]", "EU: [DE, FR, IE]\n  DACH: [DE, AT]");
    let err = PolicyService::from_yaml_str(&yaml).unwrap_err();
    assert!(
        matches!(err, PolicyConfigError::InvalidGroup { .. }),
        "{err}"
    );
}

#[test]
fn rejects_invalid_group_member() {
    let yaml = FIXTURE.replace("EU: [DE, FR, IE]", "EU: [DEU]");
    let err = PolicyService::from_yaml_str(&yaml).unwrap_err();
    assert!(
        matches!(err, PolicyConfigError::InvalidGroup { .. }),
        "{err}"
    );
}

#[test]
fn rejects_unsupported_version() {
    let yaml = FIXTURE.replace("version: 1", "version: 2");
    let err = PolicyService::from_yaml_str(&yaml).unwrap_err();
    assert!(
        matches!(err, PolicyConfigError::UnsupportedVersion { found: 2 }),
        "{err}"
    );
}
