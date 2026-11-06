//! Layered rate limiter (SR-23): token buckets in Redis/Valkey, limits from config.
//!
//! - Each named limit (`config/limits.yaml`) has a scope. `user` keys by account, falling back to
//!   the device and then the client IP when the caller is anonymous; `device` keys by device,
//!   falling back to IP; `ip` keys by client IP. So one account on five devices shares one quota.
//! - Accounts younger than `new_account.days` get `new_account_requests` instead of `requests`.
//! - An optional `global` bucket per limit is a circuit breaker across all callers.
//! - Optional `also` windows add more per-caller buckets to one limit (e.g. 20/min and 100/day);
//!   a request must pass every window.
//! - When Redis fails, `on_store_failure: closed` limits (auth, search) refuse the request;
//!   `open` limits fall back to a per-instance in-memory bucket.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use redis::aio::ConnectionManager;
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::clock::SharedClock;

const SUPPORTED_VERSION: u32 = 1;
/// Bound on every Redis call so a hung store degrades quickly instead of stalling requests.
const STORE_TIMEOUT: Duration = Duration::from_millis(300);
/// In-memory fallback buckets kept per instance before old ones are pruned.
const MEMORY_SOFT_CAP: usize = 50_000;

#[derive(Debug, thiserror::Error)]
pub enum RateLimitConfigError {
    #[error("cannot read rate limit config {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid rate limit config YAML: {0}")]
    Yaml(#[from] serde_norway::Error),
    #[error("unsupported rate limit config version {found} (supported: {SUPPORTED_VERSION})")]
    UnsupportedVersion { found: u32 },
    #[error("limit {name:?}: {reason}")]
    InvalidLimit { name: String, reason: String },
    #[error("limit {0:?} is used by a route but missing from the rate limit config")]
    Missing(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitScope {
    User,
    Device,
    Ip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailMode {
    /// Refuse requests when the store is down (auth, search).
    Closed,
    /// Fall back to an in-memory bucket per instance.
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rate {
    pub requests: u32,
    pub per_secs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitSpec {
    pub scope: LimitScope,
    pub requests: u32,
    pub per_secs: u32,
    pub on_store_failure: FailMode,
    /// Quota for accounts younger than `new_account.days` (defaults to `requests`).
    #[serde(default)]
    pub new_account_requests: Option<u32>,
    /// Circuit breaker shared by every caller of this limit.
    #[serde(default)]
    pub global: Option<Rate>,
    /// Extra per-caller windows checked after the main one (same scope; not reduced for new
    /// accounts).
    #[serde(default)]
    pub also: Vec<Rate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    new_account: NewAccount,
    limits: BTreeMap<String, LimitSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewAccount {
    days: u32,
}

/// Validated limits.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    new_account_days: u32,
    limits: HashMap<String, LimitSpec>,
}

impl RateLimitConfig {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, RateLimitConfigError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(RateLimitConfigError::UnsupportedVersion { found: raw.version });
        }
        for (name, spec) in &raw.limits {
            validate(name, spec)?;
        }
        Ok(Self {
            new_account_days: raw.new_account.days,
            limits: raw.limits.into_iter().collect(),
        })
    }

    pub fn from_path(path: &Path) -> Result<Self, RateLimitConfigError> {
        let yaml = std::fs::read_to_string(path).map_err(|source| RateLimitConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_yaml_str(&yaml)
    }

    pub fn get(&self, name: &str) -> Option<&LimitSpec> {
        self.limits.get(name)
    }

    /// Fails when a route references a limit that the config does not define.
    pub fn ensure_defined<'a>(
        &self,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), RateLimitConfigError> {
        for name in names {
            if !self.limits.contains_key(name) {
                return Err(RateLimitConfigError::Missing(name.to_owned()));
            }
        }
        Ok(())
    }
}

fn validate(name: &str, spec: &LimitSpec) -> Result<(), RateLimitConfigError> {
    let fail = |reason: &str| RateLimitConfigError::InvalidLimit {
        name: name.to_owned(),
        reason: reason.to_owned(),
    };
    if spec.requests == 0 || spec.per_secs == 0 {
        return Err(fail("requests and per_secs must be at least 1"));
    }
    if spec
        .new_account_requests
        .is_some_and(|n| n == 0 || n > spec.requests)
    {
        return Err(fail("new_account_requests must be in 1..=requests"));
    }
    if spec
        .global
        .is_some_and(|g| g.requests == 0 || g.per_secs == 0)
    {
        return Err(fail("global requests and per_secs must be at least 1"));
    }
    if spec.also.iter().any(|w| w.requests == 0 || w.per_secs == 0) {
        return Err(fail(
            "also windows need requests and per_secs of at least 1",
        ));
    }
    Ok(())
}

/// Who is calling, as far as the server knows. Never taken from request bodies.
#[derive(Debug, Clone, Default)]
pub struct Subject {
    pub account: Option<AccountSubject>,
    pub device: Option<Uuid>,
    pub ip: Option<IpAddr>,
}

#[derive(Debug, Clone, Copy)]
pub struct AccountSubject {
    pub id: Uuid,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    Limited {
        retry_after_secs: u64,
    },
    /// The store is down and the limit fails closed.
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
#[error("unknown rate limit {0:?}")]
pub struct UnknownLimit(pub String);

pub struct RateLimiter {
    config: RateLimitConfig,
    store: Store,
    memory: MemoryBuckets,
    clock: SharedClock,
    key_prefix: String,
}

enum Store {
    Redis(ConnectionManager),
    /// No shared store (tests, local tools): in-memory buckets are authoritative.
    MemoryOnly,
}

impl RateLimiter {
    pub fn with_redis(
        config: RateLimitConfig,
        redis: ConnectionManager,
        clock: SharedClock,
    ) -> Self {
        Self::new(config, Store::Redis(redis), clock)
    }

    pub fn in_memory(config: RateLimitConfig, clock: SharedClock) -> Self {
        Self::new(config, Store::MemoryOnly, clock)
    }

    fn new(config: RateLimitConfig, store: Store, clock: SharedClock) -> Self {
        Self {
            config,
            store,
            memory: MemoryBuckets::default(),
            clock,
            key_prefix: "rl".to_owned(),
        }
    }

    /// Namespaces keys (tests share one Redis).
    pub fn with_key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.key_prefix = prefix.into();
        self
    }

    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Takes one token from the caller's bucket for limit `name` (and from its global bucket).
    pub async fn check(&self, name: &str, subject: &Subject) -> Result<Decision, UnknownLimit> {
        self.check_cost(name, subject, 1).await
    }

    /// Takes `cost` tokens at once (e.g. one per signed URL in a response). A cost above the
    /// bucket capacity is always limited.
    pub async fn check_cost(
        &self,
        name: &str,
        subject: &Subject,
        cost: u32,
    ) -> Result<Decision, UnknownLimit> {
        let cost = f64::from(cost.max(1));
        let spec = self
            .config
            .get(name)
            .ok_or_else(|| UnknownLimit(name.to_owned()))?;
        let now = self.clock.now();

        if let Some(global) = spec.global {
            let key = format!("{}:{name}:global", self.key_prefix);
            let bucket = Bucket::new(global.requests, global.per_secs).with_cost(cost);
            let decision = self.take(&key, bucket, spec.on_store_failure, now).await;
            if decision != Decision::Allowed {
                return Ok(decision);
            }
        }

        let subject_key = subject_key(spec.scope, subject);
        let key = format!("{}:{name}:{subject_key}", self.key_prefix);
        let requests = match subject.account {
            Some(account) if self.is_new(account, now) => {
                spec.new_account_requests.unwrap_or(spec.requests)
            }
            _ => spec.requests,
        };
        let bucket = Bucket::new(requests, spec.per_secs).with_cost(cost);
        let decision = self.take(&key, bucket, spec.on_store_failure, now).await;
        if decision != Decision::Allowed {
            return Ok(decision);
        }
        for (index, window) in spec.also.iter().enumerate() {
            let key = format!("{}:{name}:w{index}:{subject_key}", self.key_prefix);
            let bucket = Bucket::new(window.requests, window.per_secs).with_cost(cost);
            let decision = self.take(&key, bucket, spec.on_store_failure, now).await;
            if decision != Decision::Allowed {
                return Ok(decision);
            }
        }
        Ok(Decision::Allowed)
    }

    fn is_new(&self, account: AccountSubject, now: OffsetDateTime) -> bool {
        now - account.created_at < time::Duration::days(i64::from(self.config.new_account_days))
    }

    async fn take(
        &self,
        key: &str,
        bucket: Bucket,
        fail: FailMode,
        now: OffsetDateTime,
    ) -> Decision {
        let now_ms = unix_millis(now);
        let Store::Redis(redis) = &self.store else {
            return self.memory.take(key, bucket, now_ms);
        };
        match take_in_redis(redis.clone(), key, bucket, now_ms).await {
            Ok(decision) => decision,
            Err(err) => {
                tracing::warn!(error = %err, ?fail, "rate limit store unavailable");
                match fail {
                    FailMode::Closed => Decision::Unavailable,
                    FailMode::Open => self.memory.take(key, bucket, now_ms),
                }
            }
        }
    }
}

/// `user` → account, else device, else IP; `device` → device, else IP; `ip` → IP.
fn subject_key(scope: LimitScope, subject: &Subject) -> String {
    let account = subject.account.map(|a| format!("u:{}", a.id));
    let device = subject.device.map(|d| format!("d:{d}"));
    let ip = format!(
        "ip:{}",
        subject
            .ip
            .map_or_else(|| "unknown".to_owned(), |ip| ip.to_string())
    );
    match scope {
        LimitScope::User => account.or(device).unwrap_or(ip),
        LimitScope::Device => device.unwrap_or(ip),
        LimitScope::Ip => ip,
    }
}

fn unix_millis(at: OffsetDateTime) -> i64 {
    i64::try_from(at.unix_timestamp_nanos() / 1_000_000).unwrap_or(i64::MAX)
}

/// Token bucket: `capacity` tokens, refilled continuously at `capacity / per_secs` per second.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bucket {
    capacity: f64,
    refill_per_ms: f64,
    ttl_ms: i64,
    /// Tokens one take consumes.
    cost: f64,
}

impl Bucket {
    fn new(requests: u32, per_secs: u32) -> Self {
        let per_ms = f64::from(per_secs) * 1000.0;
        Self {
            capacity: f64::from(requests),
            refill_per_ms: f64::from(requests) / per_ms,
            ttl_ms: i64::from(per_secs) * 1000 + 1000,
            cost: 1.0,
        }
    }

    fn with_cost(mut self, cost: f64) -> Self {
        self.cost = cost;
        self
    }

    /// Same arithmetic as the Lua script. Returns `(new_tokens, decision)`.
    fn take(self, tokens: Option<f64>, last_ms: Option<i64>, now_ms: i64) -> (f64, i64, Decision) {
        let mut tokens = tokens.unwrap_or(self.capacity);
        let mut last = last_ms.unwrap_or(now_ms);
        if now_ms > last {
            // Millisecond deltas are far below f64's exact integer range.
            #[allow(clippy::cast_precision_loss)]
            let elapsed = (now_ms - last) as f64;
            tokens = (tokens + elapsed * self.refill_per_ms).min(self.capacity);
            last = now_ms;
        }
        if tokens >= self.cost {
            (tokens - self.cost, last, Decision::Allowed)
        } else {
            let wait_ms = ((self.cost - tokens) / self.refill_per_ms).ceil();
            (tokens, last, limited_after_ms(wait_ms))
        }
    }
}

fn limited_after_ms(wait_ms: f64) -> Decision {
    // Bounded by the bucket period (u32 seconds), so the cast cannot overflow.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let secs = (wait_ms / 1000.0).ceil().max(1.0) as u64;
    Decision::Limited {
        retry_after_secs: secs,
    }
}

const TAKE_SCRIPT: &str = r"
local capacity = tonumber(ARGV[1])
local refill_per_ms = tonumber(ARGV[2])
local now_ms = tonumber(ARGV[3])
local ttl_ms = tonumber(ARGV[4])
local state = redis.call('HMGET', KEYS[1], 'tokens', 'last_ms')
local tokens = tonumber(state[1])
local last_ms = tonumber(state[2])
if tokens == nil then tokens = capacity end
if last_ms == nil then last_ms = now_ms end
if now_ms > last_ms then
  tokens = math.min(capacity, tokens + (now_ms - last_ms) * refill_per_ms)
  last_ms = now_ms
end
local cost = tonumber(ARGV[5])
local wait_ms = 0
if tokens >= cost then
  tokens = tokens - cost
else
  wait_ms = math.ceil((cost - tokens) / refill_per_ms)
end
redis.call('HSET', KEYS[1], 'tokens', tostring(tokens), 'last_ms', tostring(last_ms))
redis.call('PEXPIRE', KEYS[1], ttl_ms)
return wait_ms
";

async fn take_in_redis(
    mut redis: ConnectionManager,
    key: &str,
    bucket: Bucket,
    now_ms: i64,
) -> Result<Decision, String> {
    let script = redis::Script::new(TAKE_SCRIPT);
    let mut invocation = script.key(key);
    invocation
        .arg(bucket.capacity)
        .arg(bucket.refill_per_ms)
        .arg(now_ms)
        .arg(bucket.ttl_ms)
        .arg(bucket.cost);
    let call = invocation.invoke_async::<i64>(&mut redis);
    match tokio::time::timeout(STORE_TIMEOUT, call).await {
        Err(_) => Err("timed out".to_owned()),
        Ok(Err(err)) => Err(err.to_string()),
        Ok(Ok(0)) => Ok(Decision::Allowed),
        // Integer milliseconds well inside f64's exact range.
        #[allow(clippy::cast_precision_loss)]
        Ok(Ok(wait_ms)) => Ok(limited_after_ms(wait_ms as f64)),
    }
}

/// Per-instance fallback buckets.
#[derive(Default)]
struct MemoryBuckets {
    buckets: Mutex<HashMap<String, (f64, i64)>>,
}

impl MemoryBuckets {
    fn take(&self, key: &str, bucket: Bucket, now_ms: i64) -> Decision {
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if buckets.len() > MEMORY_SOFT_CAP {
            buckets.retain(|_, (_, last)| now_ms - *last < 3_600_000);
        }
        let state = buckets.get(key).copied();
        let (tokens, last, decision) = bucket.take(state.map(|s| s.0), state.map(|s| s.1), now_ms);
        buckets.insert(key.to_owned(), (tokens, last));
        decision
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::clock::{Clock, ManualClock};

    const YAML: &str = r"
version: 1
new_account:
  days: 7
limits:
  test.user:
    scope: user
    requests: 3
    per_secs: 60
    on_store_failure: open
    new_account_requests: 1
  test.ip:
    scope: ip
    requests: 2
    per_secs: 60
    on_store_failure: closed
    global: { requests: 3, per_secs: 60 }
  test.windows:
    scope: user
    requests: 3
    per_secs: 60
    on_store_failure: open
    also:
      - { requests: 4, per_secs: 3600 }
";

    fn limiter() -> (RateLimiter, Arc<ManualClock>) {
        let clock = Arc::new(ManualClock::starting_now());
        let config = RateLimitConfig::from_yaml_str(YAML).unwrap();
        (RateLimiter::in_memory(config, clock.clone()), clock)
    }

    fn old_account() -> Subject {
        Subject {
            account: Some(AccountSubject {
                id: Uuid::now_v7(),
                created_at: OffsetDateTime::UNIX_EPOCH,
            }),
            ..Subject::default()
        }
    }

    #[tokio::test]
    async fn bucket_refills_over_time_and_reports_retry_after() {
        let (limiter, clock) = limiter();
        let subject = old_account();
        for _ in 0..3 {
            assert_eq!(
                limiter.check("test.user", &subject).await.unwrap(),
                Decision::Allowed
            );
        }
        assert_eq!(
            limiter.check("test.user", &subject).await.unwrap(),
            Decision::Limited {
                retry_after_secs: 20
            }
        );
        clock.advance(time::Duration::seconds(20));
        assert_eq!(
            limiter.check("test.user", &subject).await.unwrap(),
            Decision::Allowed
        );
    }

    #[tokio::test]
    async fn costs_take_several_tokens_at_once() {
        let (limiter, clock) = limiter();
        let subject = old_account();
        assert_eq!(
            limiter.check_cost("test.user", &subject, 2).await.unwrap(),
            Decision::Allowed
        );
        // One token left: a cost of 2 waits for one more token (20 s).
        assert_eq!(
            limiter.check_cost("test.user", &subject, 2).await.unwrap(),
            Decision::Limited {
                retry_after_secs: 20
            }
        );
        assert_eq!(
            limiter.check("test.user", &subject).await.unwrap(),
            Decision::Allowed
        );
        // Above the capacity: always limited.
        clock.advance(time::Duration::seconds(120));
        assert!(matches!(
            limiter.check_cost("test.user", &subject, 4).await.unwrap(),
            Decision::Limited { .. }
        ));
    }

    #[tokio::test]
    async fn user_scope_shares_one_quota_across_devices_and_ips() {
        let (limiter, _) = limiter();
        let account = old_account().account;
        for n in 0..5u8 {
            let subject = Subject {
                account,
                device: Some(Uuid::now_v7()),
                ip: Some(IpAddr::from([10, 0, 0, n])),
            };
            let decision = limiter.check("test.user", &subject).await.unwrap();
            assert_eq!(decision == Decision::Allowed, n < 3, "device {n}");
        }
    }

    #[tokio::test]
    async fn new_accounts_get_the_reduced_quota_for_seven_days() {
        let (limiter, clock) = limiter();
        let subject = Subject {
            account: Some(AccountSubject {
                id: Uuid::now_v7(),
                created_at: clock.now(),
            }),
            ..Subject::default()
        };
        assert_eq!(
            limiter.check("test.user", &subject).await.unwrap(),
            Decision::Allowed
        );
        assert!(matches!(
            limiter.check("test.user", &subject).await.unwrap(),
            Decision::Limited { .. }
        ));
        clock.advance(time::Duration::days(7));
        // Quota lifts: bucket capacity is back to 3 once the account is 7 days old.
        for _ in 0..3 {
            assert_eq!(
                limiter.check("test.user", &subject).await.unwrap(),
                Decision::Allowed
            );
        }
    }

    #[tokio::test]
    async fn global_bucket_trips_for_everyone() {
        let (limiter, _) = limiter();
        let mut allowed = 0;
        for n in 0..5u8 {
            let subject = Subject {
                ip: Some(IpAddr::from([10, 0, 1, n])),
                ..Subject::default()
            };
            if limiter.check("test.ip", &subject).await.unwrap() == Decision::Allowed {
                allowed += 1;
            }
        }
        assert_eq!(allowed, 3, "global breaker caps all callers together");
    }

    #[tokio::test]
    async fn also_windows_cap_callers_beyond_the_main_window() {
        let (limiter, clock) = limiter();
        let subject = old_account();
        let mut allowed = 0;
        for _ in 0..3 {
            if limiter.check("test.windows", &subject).await.unwrap() == Decision::Allowed {
                allowed += 1;
            }
        }
        assert_eq!(allowed, 3);
        // The minute window refills, but the hour window (4) runs out after one more call.
        clock.advance(time::Duration::seconds(60));
        assert_eq!(
            limiter.check("test.windows", &subject).await.unwrap(),
            Decision::Allowed
        );
        assert!(matches!(
            limiter.check("test.windows", &subject).await.unwrap(),
            Decision::Limited { retry_after_secs } if retry_after_secs > 60
        ));
        let zero = YAML.replace(
            "{ requests: 4, per_secs: 3600 }",
            "{ requests: 0, per_secs: 3600 }",
        );
        assert!(RateLimitConfig::from_yaml_str(&zero).is_err());
    }

    #[tokio::test]
    async fn unknown_limit_is_an_error() {
        let (limiter, _) = limiter();
        assert!(limiter.check("nope", &Subject::default()).await.is_err());
    }

    #[test]
    fn config_rejects_invalid_values() {
        let zero = YAML.replace("requests: 3", "requests: 0");
        assert!(RateLimitConfig::from_yaml_str(&zero).is_err());
        let too_many_new = YAML.replace("new_account_requests: 1", "new_account_requests: 9");
        assert!(RateLimitConfig::from_yaml_str(&too_many_new).is_err());
        let unknown_key = YAML.replace("scope: ip", "scope: ip\n    typo: 1");
        assert!(RateLimitConfig::from_yaml_str(&unknown_key).is_err());
        let config = RateLimitConfig::from_yaml_str(YAML).unwrap();
        assert!(config.ensure_defined(["test.ip"]).is_ok());
        assert!(config.ensure_defined(["missing"]).is_err());
    }

    #[test]
    fn subject_key_falls_back_from_account_to_device_to_ip() {
        let device = Uuid::now_v7();
        let anonymous = Subject {
            device: Some(device),
            ip: Some(IpAddr::from([1, 2, 3, 4])),
            ..Subject::default()
        };
        assert_eq!(
            subject_key(LimitScope::User, &anonymous),
            format!("d:{device}")
        );
        assert_eq!(subject_key(LimitScope::Ip, &anonymous), "ip:1.2.3.4");
        assert_eq!(
            subject_key(LimitScope::Ip, &Subject::default()),
            "ip:unknown"
        );
    }
}
