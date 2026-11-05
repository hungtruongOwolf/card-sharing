//! Typed configuration from environment variables. Every value is parsed and validated once at
//! startup; an invalid or missing required value aborts startup with a message naming the key.
//!
//! Binaries build their own typed `Config` on top of [`Env`]; tests pass a map instead of
//! mutating the process environment.

use std::fmt;
use std::ops::RangeInclusive;
use std::str::FromStr;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{key} is required but not set (see .env.example)")]
    Missing { key: &'static str },
    #[error("{key}: {reason}")]
    Invalid { key: &'static str, reason: String },
}

pub fn invalid(key: &'static str, reason: impl Into<String>) -> ConfigError {
    ConfigError::Invalid {
        key,
        reason: reason.into(),
    }
}

/// Reads keys through any lookup function (`std::env::var` in production, a map in tests).
/// Values are trimmed; empty values count as unset.
pub struct Env<F> {
    lookup: F,
}

impl Env<fn(&str) -> Option<String>> {
    /// Reads the process environment.
    pub fn process() -> Self {
        Self {
            lookup: |key| std::env::var(key).ok(),
        }
    }
}

impl<F: Fn(&str) -> Option<String>> Env<F> {
    pub fn new(lookup: F) -> Self {
        Self { lookup }
    }

    pub fn optional(&self, key: &str) -> Option<String> {
        (self.lookup)(key)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    pub fn required(&self, key: &'static str) -> Result<String, ConfigError> {
        self.optional(key).ok_or(ConfigError::Missing { key })
    }

    /// A required secret (password, key, connection string). Never printed by `Debug`.
    pub fn required_secret(&self, key: &'static str) -> Result<Secret, ConfigError> {
        self.required(key).map(Secret::new)
    }

    /// Parses `key`, or returns `default` when unset.
    pub fn parse_or<T: FromStr>(&self, key: &'static str, default: T) -> Result<T, ConfigError> {
        match self.optional(key) {
            None => Ok(default),
            Some(raw) => raw
                .parse()
                .map_err(|_| invalid(key, format!("{raw:?} has the wrong format"))),
        }
    }

    /// Parses a number in `range`, or returns `default` when unset.
    pub fn in_range<T>(
        &self,
        key: &'static str,
        default: T,
        range: RangeInclusive<T>,
    ) -> Result<T, ConfigError>
    where
        T: FromStr + PartialOrd + fmt::Display,
    {
        let Some(raw) = self.optional(key) else {
            return Ok(default);
        };
        let value: T = raw
            .parse()
            .map_err(|_| invalid(key, format!("{raw:?} is not a number")))?;
        if !range.contains(&value) {
            return Err(invalid(
                key,
                format!("{value} is outside {}..={}", range.start(), range.end()),
            ));
        }
        Ok(value)
    }

    /// Parses `true`/`false`, or returns `default` when unset.
    pub fn flag(&self, key: &'static str, default: bool) -> Result<bool, ConfigError> {
        match self.optional(key).as_deref() {
            None => Ok(default),
            Some("true") => Ok(true),
            Some("false") => Ok(false),
            Some(other) => Err(invalid(key, format!("{other:?} must be true or false"))),
        }
    }
}

/// A configuration secret. `Debug`/`Display` never reveal the value, so configs can derive
/// `Debug` and be logged safely.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The raw value. Pass it straight to the client library that needs it; never log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Env<impl Fn(&str) -> Option<String>> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Env::new(move |key| map.get(key).cloned())
    }

    #[test]
    fn missing_required_value_names_the_key() {
        let err = env(&[]).required("DATABASE_URL").unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Missing {
                key: "DATABASE_URL"
            }
        ));
        assert!(err.to_string().contains("DATABASE_URL"));
    }

    #[test]
    fn blank_values_count_as_unset() {
        assert!(env(&[("KEY", "   ")]).optional("KEY").is_none());
        assert!(env(&[("KEY", "  ")]).required("KEY").is_err());
    }

    #[test]
    fn in_range_uses_default_and_rejects_out_of_range() {
        let e = env(&[("N", "500"), ("BAD", "x")]);
        assert_eq!(e.in_range("UNSET", 7u32, 1..=10).unwrap(), 7);
        assert!(e.in_range("N", 7u32, 1..=10).is_err());
        assert!(e.in_range("BAD", 7u32, 1..=10).is_err());
    }

    #[test]
    fn flag_accepts_only_true_and_false() {
        let e = env(&[("A", "true"), ("B", "yes")]);
        assert!(e.flag("A", false).unwrap());
        assert!(!e.flag("UNSET", false).unwrap());
        assert!(e.flag("B", false).is_err());
    }

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let secret = env(&[("PW", "hunter2")]).required_secret("PW").unwrap();
        assert_eq!(secret.expose(), "hunter2");
        assert!(!format!("{secret:?}").contains("hunter2"));
    }
}
