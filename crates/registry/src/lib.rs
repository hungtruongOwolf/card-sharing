//! Server copy of the effects registry (T-60, SR-28; context/wiki/product/effects-catalog.md).
//!
//! The registry is defined once, in `packages/effects-registry` (TypeScript). Its build step
//! exports ids, versions, kinds, stacking rules and param ranges to
//! `packages/effects-registry/generated/registry.json` (a JS test fails CI when that file is stale);
//! this crate embeds the file at compile time, so the API always validates against the registry it
//! was built with.
//!
//! [`Registry::validate_style`] checks a card's `style` block
//! (`{cut?, finish?, effects?: [...]}`, each `{id, version, params?}`): unknown ids or versions,
//! wrong kinds, duplicates of non-stackable entries, conflicting entries, unknown params and
//! out-of-range or mistyped values are all refused with one generic error
//! (422 `UNKNOWN_REGISTRY_ID` at the HTTP layer). Missing params take their defaults, so the
//! returned style is complete and safe to store.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use serde_json::{Map, Value};

/// The exported registry the API was built with.
pub const REGISTRY_JSON: &str =
    include_str!("../../../packages/effects-registry/generated/registry.json");
const SCHEMA_VERSION: u32 = 1;
/// Upper bound on stacked effects in one style block.
pub const MAX_EFFECTS: usize = 8;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportedRegistry {
    schema_version: u32,
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub version: u32,
    pub kind: Kind,
    pub stackable: bool,
    pub conflicts_with: Vec<String>,
    pub params: BTreeMap<String, Param>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    CutStyle,
    Filter,
    Overlay,
    Prop,
    Frame,
    Layout,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Param {
    Number {
        min: f64,
        max: f64,
        default: f64,
    },
    Integer {
        min: i64,
        max: i64,
        default: i64,
    },
    Boolean {
        default: bool,
    },
    Color {
        default: String,
    },
    Enum {
        values: Vec<String>,
        default: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// The exported JSON does not have the expected shape (a build/CI problem, not client input).
    #[error("registry export is invalid: {0}")]
    InvalidExport(String),
    /// The style block references something the registry does not allow. `reason` is for logs;
    /// clients get one generic error.
    #[error("style rejected: {reason}")]
    Rejected { reason: String },
}

fn reject(reason: impl Into<String>) -> RegistryError {
    RegistryError::Rejected {
        reason: reason.into(),
    }
}

pub struct Registry {
    entries: HashMap<String, Entry>,
}

impl Registry {
    /// The registry embedded at build time.
    pub fn embedded() -> Result<Self, RegistryError> {
        Self::from_json(REGISTRY_JSON)
    }

    /// A registry with no entries: every style that names an entry is refused.
    pub fn empty() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn from_json(json: &str) -> Result<Self, RegistryError> {
        let export: ExportedRegistry = serde_json::from_str(json)
            .map_err(|err| RegistryError::InvalidExport(err.to_string()))?;
        if export.schema_version != SCHEMA_VERSION {
            return Err(RegistryError::InvalidExport(format!(
                "schema_version {} (expected {SCHEMA_VERSION})",
                export.schema_version
            )));
        }
        let mut entries = HashMap::new();
        for entry in export.entries {
            if entries.insert(entry.id.clone(), entry).is_some() {
                return Err(RegistryError::InvalidExport("duplicate id".to_owned()));
            }
        }
        Ok(Self { entries })
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.get(id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Validates a `style` block and returns it normalized (every param present, nothing else).
    pub fn validate_style(&self, style: &Value) -> Result<Value, RegistryError> {
        let Value::Object(block) = style else {
            return Err(reject("style must be an object"));
        };
        let mut out = Map::new();
        let mut chosen: Vec<&Entry> = Vec::new();
        for (key, value) in block {
            match key.as_str() {
                "cut" => {
                    let (entry, normalized) = self.reference(value, |k| k == Kind::CutStyle)?;
                    chosen.push(entry);
                    out.insert(key.clone(), normalized);
                }
                "finish" => {
                    let (entry, normalized) = self.reference(value, |k| k == Kind::Filter)?;
                    chosen.push(entry);
                    out.insert(key.clone(), normalized);
                }
                "effects" => {
                    let Value::Array(items) = value else {
                        return Err(reject("effects must be an array"));
                    };
                    if items.len() > MAX_EFFECTS {
                        return Err(reject("too many effects"));
                    }
                    let mut normalized = Vec::with_capacity(items.len());
                    for item in items {
                        let (entry, value) = self.reference(item, |k| k != Kind::CutStyle)?;
                        chosen.push(entry);
                        normalized.push(value);
                    }
                    out.insert(key.clone(), Value::Array(normalized));
                }
                other => return Err(reject(format!("unknown style field {other}"))),
            }
        }
        check_combination(&chosen)?;
        Ok(Value::Object(out))
    }

    /// One `{id, version, params?}` reference of an allowed kind.
    fn reference(
        &self,
        value: &Value,
        kind_allowed: impl Fn(Kind) -> bool,
    ) -> Result<(&Entry, Value), RegistryError> {
        let Value::Object(fields) = value else {
            return Err(reject("reference must be an object"));
        };
        if let Some(extra) = fields
            .keys()
            .find(|k| !matches!(k.as_str(), "id" | "version" | "params"))
        {
            return Err(reject(format!("unknown reference field {extra}")));
        }
        let id = fields
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| reject("id missing"))?;
        let entry = self
            .entries
            .get(id)
            .ok_or_else(|| reject(format!("unknown id {id}")))?;
        let version = fields.get("version").and_then(Value::as_u64);
        if version != Some(u64::from(entry.version)) {
            return Err(reject(format!("unknown version of {id}")));
        }
        if !kind_allowed(entry.kind) {
            return Err(reject(format!("{id} is not allowed in this slot")));
        }
        let params = normalize_params(entry, fields.get("params"))?;
        let mut normalized = Map::new();
        normalized.insert("id".to_owned(), Value::from(entry.id.clone()));
        normalized.insert("version".to_owned(), Value::from(entry.version));
        normalized.insert("params".to_owned(), Value::Object(params));
        Ok((entry, Value::Object(normalized)))
    }
}

/// Non-stackable entries appear once; no two chosen entries conflict.
fn check_combination(chosen: &[&Entry]) -> Result<(), RegistryError> {
    for (i, a) in chosen.iter().enumerate() {
        for b in &chosen[i + 1..] {
            if a.id == b.id && !a.stackable {
                return Err(reject(format!("{} is not stackable", a.id)));
            }
            if a.conflicts_with.contains(&b.id) || b.conflicts_with.contains(&a.id) {
                return Err(reject(format!("{} conflicts with {}", a.id, b.id)));
            }
        }
    }
    Ok(())
}

/// Strict params: unknown names and invalid values are errors; missing ones take defaults.
fn normalize_params(
    entry: &Entry,
    given: Option<&Value>,
) -> Result<Map<String, Value>, RegistryError> {
    let empty = Map::new();
    let given = match given {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(map)) => map,
        Some(_) => return Err(reject("params must be an object")),
    };
    if let Some(unknown) = given.keys().find(|name| !entry.params.contains_key(*name)) {
        return Err(reject(format!("unknown param {unknown} of {}", entry.id)));
    }
    let mut out = Map::new();
    for (name, spec) in &entry.params {
        let value = match given.get(name) {
            Some(value) => check_value(spec, value)
                .ok_or_else(|| reject(format!("invalid value for {name} of {}", entry.id)))?,
            None => default_value(spec),
        };
        out.insert(name.clone(), value);
    }
    Ok(out)
}

fn check_value(spec: &Param, value: &Value) -> Option<Value> {
    match spec {
        Param::Number { min, max, .. } => {
            let n = value.as_f64()?;
            (n.is_finite() && n >= *min && n <= *max).then(|| value.clone())
        }
        Param::Integer { min, max, .. } => {
            let n = value.as_i64()?;
            (n >= *min && n <= *max).then(|| value.clone())
        }
        Param::Boolean { .. } => value.is_boolean().then(|| value.clone()),
        Param::Color { .. } => {
            let s = value.as_str()?;
            is_color(s).then(|| value.clone())
        }
        Param::Enum { values, .. } => {
            let s = value.as_str()?;
            values.iter().any(|v| v == s).then(|| value.clone())
        }
    }
}

fn default_value(spec: &Param) -> Value {
    match spec {
        Param::Number { default, .. } => Value::from(*default),
        Param::Integer { default, .. } => Value::from(*default),
        Param::Boolean { default } => Value::from(*default),
        Param::Color { default } | Param::Enum { default, .. } => Value::from(default.clone()),
    }
}

/// `#RRGGBB` or `#RRGGBBAA`, any hex case (same rule as the app).
fn is_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn registry() -> Registry {
        Registry::embedded().unwrap()
    }

    #[test]
    fn the_committed_export_loads() {
        let registry = registry();
        assert!(!registry.is_empty());
        let diecut = registry.get("cut.diecut").unwrap();
        assert_eq!(diecut.kind, Kind::CutStyle);
        assert!(diecut.params.contains_key("borderWidthPct"));
    }

    #[test]
    fn a_valid_style_is_normalized_with_defaults() {
        let style = json!({
            "cut": { "id": "cut.diecut", "version": 1, "params": { "borderWidthPct": 4 } },
            "effects": [{ "id": "fx.holo", "version": 1 }],
        });
        let normalized = registry().validate_style(&style).unwrap();
        assert_eq!(normalized["cut"]["params"]["borderWidthPct"], 4);
        assert_eq!(normalized["cut"]["params"]["color"], "#FFFFFF");
        assert_eq!(normalized["effects"][0]["params"]["intensity"], 0.8);
    }

    #[test]
    fn unknown_ids_versions_and_out_of_range_params_are_rejected() {
        let registry = registry();
        for style in [
            json!({ "effects": [{ "id": "fx.unknown", "version": 1 }] }),
            json!({ "cut": { "id": "cut.diecut", "version": 2 } }),
            json!({ "cut": { "id": "cut.diecut", "version": 1, "params": { "borderWidthPct": 1e308 } } }),
            json!({ "cut": { "id": "cut.diecut", "version": 1, "params": { "borderWidthPct": "3" } } }),
            json!({ "cut": { "id": "cut.diecut", "version": 1, "params": { "color": "red" } } }),
            json!({ "cut": { "id": "cut.diecut", "version": 1, "params": { "evil": 1 } } }),
            json!({ "cut": { "id": "cut.diecut", "version": 1, "owner_id": "x" } }),
            json!({ "cut": { "id": "fx.holo", "version": 1 } }),
            json!({ "finish": { "id": "cut.none", "version": 1 } }),
            json!({ "effects": [{ "id": "cut.raw", "version": 1 }] }),
            json!({ "sticker": { "id": "cut.raw", "version": 1 } }),
            json!({ "effects": { "id": "fx.holo", "version": 1 } }),
            json!([]),
        ] {
            assert!(
                matches!(
                    registry.validate_style(&style),
                    Err(RegistryError::Rejected { .. })
                ),
                "{style} must be rejected"
            );
        }
    }

    #[test]
    fn stacking_and_conflict_rules_apply() {
        let json = r##"{"schema_version":1,"entries":[
            {"id":"fx.a","version":1,"kind":"filter","stackable":false,"conflicts_with":["fx.b"],"params":{}},
            {"id":"fx.b","version":1,"kind":"filter","stackable":true,"conflicts_with":[],"params":{}},
            {"id":"fx.n","version":1,"kind":"filter","stackable":true,"conflicts_with":[],
             "params":{"n":{"type":"integer","min":0,"max":3,"default":1},
                       "mode":{"type":"enum","values":["a","b"],"default":"a"},
                       "on":{"type":"boolean","default":true}}}]}"##;
        let registry = Registry::from_json(json).unwrap();
        let reference = |id: &str| json!({ "id": id, "version": 1 });
        let style = |items: Vec<Value>| json!({ "effects": items });
        assert!(
            registry
                .validate_style(&style(vec![reference("fx.b"), reference("fx.b")]))
                .is_ok()
        );
        assert!(
            registry
                .validate_style(&style(vec![reference("fx.a"), reference("fx.a")]))
                .is_err()
        );
        assert!(
            registry
                .validate_style(&style(vec![reference("fx.b"), reference("fx.a")]))
                .is_err()
        );
        let too_many = style((0..=MAX_EFFECTS).map(|_| reference("fx.b")).collect());
        assert!(registry.validate_style(&too_many).is_err());
        let n = |params: Value| {
            style(vec![
                json!({ "id": "fx.n", "version": 1, "params": params }),
            ])
        };
        assert!(
            registry
                .validate_style(&n(json!({ "n": 3, "mode": "b", "on": false })))
                .is_ok()
        );
        assert!(registry.validate_style(&n(json!({ "n": 1.5 }))).is_err());
        assert!(registry.validate_style(&n(json!({ "n": 4 }))).is_err());
        assert!(registry.validate_style(&n(json!({ "mode": "c" }))).is_err());
        assert!(registry.validate_style(&n(json!({ "on": 1 }))).is_err());
    }

    #[test]
    fn malformed_exports_are_refused() {
        assert!(Registry::from_json("{}").is_err());
        assert!(Registry::from_json(r#"{"schema_version":2,"entries":[]}"#).is_err());
        assert!(
            Registry::from_json(
                r#"{"schema_version":1,"entries":[{"id":"fx.a","version":1,"kind":"filter","stackable":true,"conflicts_with":[],"params":{},"extra":1}]}"#
            )
            .is_err()
        );
    }
}
