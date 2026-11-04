use std::fmt;
use std::str::FromStr;

/// ISO 3166-1 alpha-2 country code, stored uppercase (e.g. `DE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CountryCode([u8; 2]);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid country code {0:?}: expected two ASCII letters (ISO 3166-1 alpha-2)")]
pub struct InvalidCountryCode(pub String);

impl CountryCode {
    pub fn as_str(&self) -> &str {
        // Invariant: both bytes are ASCII uppercase letters (checked in `from_str`).
        std::str::from_utf8(&self.0).unwrap_or("??")
    }
}

impl FromStr for CountryCode {
    type Err = InvalidCountryCode;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.as_bytes() {
            [a, b] if a.is_ascii_alphabetic() && b.is_ascii_alphabetic() => {
                Ok(Self([a.to_ascii_uppercase(), b.to_ascii_uppercase()]))
            }
            _ => Err(InvalidCountryCode(value.to_owned())),
        }
    }
}

impl fmt::Display for CountryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_uppercases() {
        let code: CountryCode = "de".parse().unwrap();
        assert_eq!(code.as_str(), "DE");
    }

    #[test]
    fn rejects_wrong_length_and_non_letters() {
        for bad in ["", "D", "DEU", "D1", "ü1", "  "] {
            assert!(
                bad.parse::<CountryCode>().is_err(),
                "{bad:?} should be rejected"
            );
        }
    }
}
