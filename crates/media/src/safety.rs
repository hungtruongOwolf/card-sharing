//! Content-safety ports (ADR-0005, content-safety.md): the known-CSAM `HashMatcher` (T-72).
//! Real vendors wait for the EM's decisions; until then
//! only the `unconfigured` adapters (always unavailable, so nothing is ever delivered) and the
//! deterministic `fake` adapters (development, test and staging only; refused in production
//! at startup) exist. Every error, timeout or unknown answer is treated as "unavailable" by the
//! pipeline: the media stays `scanning` and the job retries with backoff (fail closed).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What the gates look at: the decoded, re-encoded image (never the raw upload).
pub struct ScanInput<'a> {
    /// SHA-256 of the re-encoded full-size image.
    pub sha256: [u8; 32],
    pub width: u32,
    pub height: u32,
    /// RGBA8 pixels, row major.
    pub rgba: &'a [u8],
}

/// Category of a known-CSAM list match (content-safety.md "port changes").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchCategory {
    Csam,
    HarmfulAbusive,
    /// A vendor's test image (end-to-end tests without illegal material).
    Test,
}

impl MatchCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csam => "csam",
            Self::HarmfulAbusive => "harmful_abusive",
            Self::Test => "test",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashVerdict {
    NoMatch {
        provider: String,
        list_version: String,
    },
    Match {
        category: MatchCategory,
        exact: bool,
        provider: String,
        list_version: String,
        /// The provider's reference for the matched entry (stored for the report).
        provider_ref: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("content-safety provider unavailable: {0}")]
pub struct Unavailable(pub String);

pub trait HashMatcher: Send + Sync {
    fn check<'a>(
        &'a self,
        input: &'a ScanInput<'a>,
    ) -> BoxFuture<'a, Result<HashVerdict, Unavailable>>;
}

/// Top-left pixel that the fake matcher treats as a known-CSAM **test** match (magenta).
pub const FAKE_MATCH_MARKER: [u8; 4] = [255, 0, 255, 255];
/// Top-left pixel that makes the fake matcher fail as unavailable (cyan).
pub const FAKE_MATCHER_DOWN_MARKER: [u8; 4] = [0, 255, 255, 255];

fn top_left(input: &ScanInput<'_>) -> Option<[u8; 4]> {
    let px = input.rgba.get(..4)?;
    Some([px[0], px[1], px[2], px[3]])
}

/// No matcher configured: every check is unavailable, so nothing is ever delivered.
pub struct UnconfiguredHashMatcher;

impl HashMatcher for UnconfiguredHashMatcher {
    fn check<'a>(
        &'a self,
        _: &'a ScanInput<'a>,
    ) -> BoxFuture<'a, Result<HashVerdict, Unavailable>> {
        Box::pin(async { Err(Unavailable("no CSAM hash matcher is configured".to_owned())) })
    }
}

/// Deterministic stand-in for development, tests and staging (refused in production): a
/// magenta top-left pixel is a `Test` match, a cyan one an outage, anything else no match.
pub struct FakeHashMatcher;

impl HashMatcher for FakeHashMatcher {
    fn check<'a>(
        &'a self,
        input: &'a ScanInput<'a>,
    ) -> BoxFuture<'a, Result<HashVerdict, Unavailable>> {
        let verdict = match top_left(input) {
            Some(FAKE_MATCH_MARKER) => Ok(HashVerdict::Match {
                category: MatchCategory::Test,
                exact: true,
                provider: "fake".to_owned(),
                list_version: "fake-1".to_owned(),
                provider_ref: "fake-test-entry".to_owned(),
            }),
            Some(FAKE_MATCHER_DOWN_MARKER) => Err(Unavailable("fake matcher outage".to_owned())),
            _ => Ok(HashVerdict::NoMatch {
                provider: "fake".to_owned(),
                list_version: "fake-1".to_owned(),
            }),
        };
        Box::pin(async move { verdict })
    }
}

/// The gates the pipeline runs, with their time budget.
#[derive(Clone)]
pub struct ContentGates {
    pub matcher: Arc<dyn HashMatcher>,
    /// Upper bound on one provider call; a slower answer counts as unavailable.
    pub timeout: Duration,
}

impl ContentGates {
    /// Unconfigured gates: every scan is unavailable, nothing is ever delivered.
    pub fn unconfigured() -> Self {
        Self {
            matcher: Arc::new(UnconfiguredHashMatcher),
            timeout: Duration::from_secs(10),
        }
    }

    /// The hash matcher's verdict; errors and timeouts are [`Unavailable`].
    pub async fn hash_verdict(&self, input: &ScanInput<'_>) -> Result<HashVerdict, Unavailable> {
        match tokio::time::timeout(self.timeout, self.matcher.check(input)).await {
            Ok(result) => result,
            Err(_) => Err(Unavailable("hash matcher timed out".to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn input(top_left: [u8; 4]) -> Vec<u8> {
        let mut rgba = top_left.to_vec();
        rgba.extend_from_slice(&[1, 2, 3, 255]);
        rgba
    }

    fn scan(rgba: &[u8]) -> ScanInput<'_> {
        ScanInput {
            sha256: [0; 32],
            width: 2,
            height: 1,
            rgba,
        }
    }

    #[tokio::test]
    async fn fake_matcher_follows_its_markers() {
        let gates = ContentGates {
            matcher: Arc::new(FakeHashMatcher),
            timeout: Duration::from_secs(1),
        };
        let clean = input([10, 20, 30, 255]);
        assert!(matches!(
            gates.hash_verdict(&scan(&clean)).await,
            Ok(HashVerdict::NoMatch { .. })
        ));
        let hit = input(FAKE_MATCH_MARKER);
        assert!(matches!(
            gates.hash_verdict(&scan(&hit)).await,
            Ok(HashVerdict::Match {
                category: MatchCategory::Test,
                ..
            })
        ));
        let down = input(FAKE_MATCHER_DOWN_MARKER);
        assert!(gates.hash_verdict(&scan(&down)).await.is_err());
    }

    #[tokio::test]
    async fn unconfigured_and_slow_matchers_are_unavailable() {
        let clean = input([10, 20, 30, 255]);
        assert!(
            ContentGates::unconfigured()
                .hash_verdict(&scan(&clean))
                .await
                .is_err()
        );

        struct Slow;
        impl HashMatcher for Slow {
            fn check<'a>(
                &'a self,
                _: &'a ScanInput<'a>,
            ) -> BoxFuture<'a, Result<HashVerdict, Unavailable>> {
                Box::pin(async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok(HashVerdict::NoMatch {
                        provider: "slow".to_owned(),
                        list_version: "1".to_owned(),
                    })
                })
            }
        }
        let gates = ContentGates {
            matcher: Arc::new(Slow),
            timeout: Duration::from_millis(10),
        };
        assert!(gates.hash_verdict(&scan(&clean)).await.is_err());
    }
}
