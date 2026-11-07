//! AI effect rules from `config/ai_effects.yaml` (config over code): consent version, starter
//! credits and the curated templates.

use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;

const SUPPORTED_VERSION: u32 = 1;
/// Most templates one config may list (they are shown in one picker).
const MAX_TEMPLATES: usize = 50;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("could not read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid AI effects config: {0}")]
    Parse(#[from] serde_norway::Error),
    #[error("unsupported AI effects config version {found}")]
    UnsupportedVersion { found: u32 },
    #[error("invalid AI effects config: {0}")]
    Invalid(String),
}

/// What the fake provider does to the image (it never calls a network service).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FakeTransform {
    Invert,
    Grayscale,
    Posterize,
}

/// One curated template. Clients name it by `id` + `version`; a new prompt is a new version.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub id: String,
    pub version: u32,
    pub title: String,
    /// Credits one run costs (reserved at submit, refunded on refusal or failure).
    pub cost: u32,
    pub enabled: bool,
    /// For real providers; never shown to clients and never taken from them.
    pub prompt: String,
    pub fake_transform: FakeTransform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenAiSettings {
    pub consent_version: u32,
    pub starter_credits: u32,
    pub templates: Vec<Template>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    consent_version: u32,
    starter_credits: u32,
    templates: Vec<Template>,
}

impl GenAiSettings {
    /// The committed `config/ai_effects.yaml` (checked by tests). An invalid file yields no
    /// templates, so nothing can be submitted (fails closed) instead of crashing.
    pub fn shipped() -> Self {
        Self::from_yaml_str(include_str!("../../../config/ai_effects.yaml")).unwrap_or_else(|err| {
            tracing::error!(error = %err, "embedded AI effects config is invalid");
            Self {
                consent_version: 1,
                starter_credits: 0,
                templates: Vec::new(),
            }
        })
    }

    pub fn from_yaml_str(yaml: &str) -> Result<Self, SettingsError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(SettingsError::UnsupportedVersion { found: raw.version });
        }
        let invalid = |reason: &str| SettingsError::Invalid(reason.to_owned());
        if raw.consent_version == 0 {
            return Err(invalid("consent_version must be at least 1"));
        }
        if raw.starter_credits > 1000 {
            return Err(invalid("starter_credits must be at most 1000"));
        }
        if raw.templates.len() > MAX_TEMPLATES {
            return Err(invalid("too many templates"));
        }
        let mut ids = HashSet::new();
        for template in &raw.templates {
            let id_ok = template.id.starts_with("ai.")
                && template.id.len() <= 64
                && template.id[3..]
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                && template.id.len() > 3;
            if !id_ok {
                return Err(invalid("template ids look like ai.<lowercase_name>"));
            }
            if !ids.insert(template.id.as_str()) {
                return Err(invalid("template ids must be unique"));
            }
            if template.version == 0 || template.cost == 0 || template.cost > 100 {
                return Err(invalid("template version >= 1 and cost in 1..=100"));
            }
            if template.title.trim().is_empty() || template.title.chars().count() > 40 {
                return Err(invalid("template titles have 1 to 40 characters"));
            }
            if template.prompt.trim().is_empty() {
                return Err(invalid("templates need a prompt"));
            }
        }
        Ok(Self {
            consent_version: raw.consent_version,
            starter_credits: raw.starter_credits,
            templates: raw.templates,
        })
    }

    pub fn from_path(path: &Path) -> Result<Self, SettingsError> {
        let yaml = std::fs::read_to_string(path).map_err(|source| SettingsError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_yaml_str(&yaml)
    }

    /// The enabled template with exactly this id and version.
    pub fn template(&self, id: &str, version: u32) -> Option<&Template> {
        self.templates
            .iter()
            .find(|t| t.enabled && t.id == id && t.version == version)
    }

    /// Templates offered to clients.
    pub fn enabled_templates(&self) -> impl Iterator<Item = &Template> {
        self.templates.iter().filter(|t| t.enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_config_is_valid() {
        let settings = GenAiSettings::shipped();
        assert!(settings.enabled_templates().count() >= 1);
        assert!(settings.template("ai.watercolor", 1).is_some());
        assert!(settings.template("ai.watercolor", 2).is_none());
    }

    #[test]
    fn invalid_configs_are_refused() {
        let yaml = include_str!("../../../config/ai_effects.yaml");
        for (from, to) in [
            ("version: 1\n", "version: 2\n"),
            ("consent_version: 1", "consent_version: 0"),
            ("id: ai.neon_night", "id: ai.watercolor"),
            ("id: ai.neon_night", "id: neon"),
            ("cost: 2", "cost: 0"),
            ("fake_transform: invert", "fake_transform: swap_faces"),
            (
                "enabled: true\n    prompt: Give",
                "enabled: true\n    typo: 1\n    prompt: Give",
            ),
        ] {
            assert!(yaml.contains(from), "{from}");
            let broken = yaml.replacen(from, to, 1);
            assert!(GenAiSettings::from_yaml_str(&broken).is_err(), "{to}");
        }
    }
}
