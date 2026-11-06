//! Captcha verification port for guest reports (T-142, AC-26.3). The provider is still an EM
//! decision (privacy-friendly preferred), so only two adapters exist:
//!
//! - [`NoCaptcha`]: no provider configured; every check is unavailable, so guest reports answer
//!   503 (fail closed).
//! - [`FakeCaptcha`]: deterministic test adapter; refused at startup when `API_ENV=production`.
//!
//! A real adapter verifies the widget token with the provider's server-side API.

use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;

/// The token [`FakeCaptcha`] accepts.
pub const FAKE_CAPTCHA_PASS: &str = "fake-captcha-pass";
/// The token for which [`FakeCaptcha`] simulates a provider outage.
pub const FAKE_CAPTCHA_OUTAGE: &str = "fake-captcha-outage";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptchaOutcome {
    Passed,
    /// Wrong, expired, replayed or malformed token.
    Failed,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptchaError {
    #[error("captcha verification unavailable: {0}")]
    Unavailable(&'static str),
}

pub type CaptchaFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CaptchaOutcome, CaptchaError>> + Send + 'a>>;

pub trait CaptchaVerifier: Send + Sync {
    /// Verifies a widget token the visitor's browser obtained. `client_ip` is the visitor's IP
    /// as the web server saw it (providers may compare it).
    fn verify<'a>(&'a self, token: &'a str, client_ip: Option<IpAddr>) -> CaptchaFuture<'a>;
}

/// No provider configured: verification is always unavailable.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoCaptcha;

impl CaptchaVerifier for NoCaptcha {
    fn verify<'a>(&'a self, _token: &'a str, _client_ip: Option<IpAddr>) -> CaptchaFuture<'a> {
        Box::pin(async { Err(CaptchaError::Unavailable("no captcha provider configured")) })
    }
}

/// Development and test adapter: [`FAKE_CAPTCHA_PASS`] passes, [`FAKE_CAPTCHA_OUTAGE`] is an
/// outage, anything else fails.
#[derive(Debug, Default, Clone, Copy)]
pub struct FakeCaptcha;

impl CaptchaVerifier for FakeCaptcha {
    fn verify<'a>(&'a self, token: &'a str, _client_ip: Option<IpAddr>) -> CaptchaFuture<'a> {
        Box::pin(async move {
            match token {
                FAKE_CAPTCHA_PASS => Ok(CaptchaOutcome::Passed),
                FAKE_CAPTCHA_OUTAGE => Err(CaptchaError::Unavailable("fake outage")),
                _ => Ok(CaptchaOutcome::Failed),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<F: Future>(future: F) -> F::Output {
        // The adapters never await anything, so one poll finishes them.
        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);
        let mut future = Box::pin(future);
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => value,
            std::task::Poll::Pending => unreachable!("captcha adapters are synchronous"),
        }
    }

    #[test]
    fn fake_and_none_behave_as_documented() {
        let fake = FakeCaptcha;
        assert_eq!(
            block_on(fake.verify(FAKE_CAPTCHA_PASS, None)).unwrap(),
            CaptchaOutcome::Passed
        );
        assert_eq!(
            block_on(fake.verify("nope", None)).unwrap(),
            CaptchaOutcome::Failed
        );
        assert!(block_on(fake.verify(FAKE_CAPTCHA_OUTAGE, None)).is_err());
        assert!(block_on(NoCaptcha.verify(FAKE_CAPTCHA_PASS, None)).is_err());
    }
}
