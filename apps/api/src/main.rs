use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use api::client_config::ClientConfig;
use api::config::Config;
use api::state::{AppState, ProfileSettings};
use identity::apple::{
    AppleClientSecretSigner, AppleHttpClient, AppleTokenClient, UnconfiguredAppleClient,
};
use identity::oidc::{IdTokenVerifier, JwksSettings, ProviderSpec, VerifierSettings};
use identity::{IdentityService, ProfileRules, ProviderSignIn, SessionPolicy};
use platform::clock::{SharedClock, SystemClock};
use platform::crypto::FieldCipher;
use platform::idempotency::{IdempotencySettings, IdempotencyStore};
use platform::jobs::{JobQueue, PeriodicFuture, PeriodicTask, Worker, WorkerSettings};
use platform::outbox::{OutboxRelay, RelayTask};
use platform::rate_limit::{RateLimitConfig, RateLimiter};
use platform::text::TextFilter;
use policy::PolicyService;
use tokio::net::TcpListener;

/// `api` serves HTTP; `api migrate` applies database migrations and exits (run it from the
/// deploy pipeline before rolling out a new version); `api worker` runs the job worker
/// (ADR-0006) until SIGTERM.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("invalid configuration")?;
    api::telemetry::init(config.log_format).context("failed to initialise logging")?;

    let pool = api::db::connect_lazy(&config.database_url, config.db_max_connections)
        .context("invalid DATABASE_URL")?;
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("migrate") => {
            api::db::MIGRATOR
                .run(&pool)
                .await
                .context("database migration failed")?;
            tracing::info!("migrations applied");
            return Ok(());
        }
        Some("worker") => {
            run_worker(pool).await;
            return Ok(());
        }
        Some(other) => {
            anyhow::bail!("unknown command {other:?} (expected no argument, `migrate` or `worker`)")
        }
    }

    let policy = PolicyService::from_path(&config.policy_config_path)
        .context("failed to load region policy config")?;
    tracing::info!(
        version = policy.config_version(),
        "region policy config loaded"
    );

    let limits = RateLimitConfig::from_path(&config.limits_config_path)
        .context("failed to load rate limit config")?;
    let (_, _, route_policies) = api::openapi::router().finish();
    limits
        .ensure_defined(route_policies.rate_limits())
        .context("rate limit config does not cover every route")?;
    let redis = platform::cache::connect_lazy(&config.redis_url).context("invalid REDIS_URL")?;
    let clock: SharedClock = Arc::new(SystemClock);
    let limiter = RateLimiter::with_redis(limits, redis.clone(), Arc::clone(&clock));
    let identity = Arc::new(IdentityService::new(
        pool,
        Some(redis),
        Arc::clone(&clock),
        SessionPolicy::default(),
    ));

    let client = ClientConfig::from_path(&config.client_config_path)
        .context("failed to load client config")?;
    let field_cipher = FieldCipher::from_base64(1, config.data_encryption_key.expose())
        .context("invalid API_DATA_ENCRYPTION_KEY")?;
    let profile = ProfileSettings {
        text_filter: TextFilter::from_path(&config.text_filter_config_path)
            .context("failed to load text filter config")?,
        rules: ProfileRules::from_path(&config.profile_config_path)
            .context("failed to load profile config")?,
    };
    let state = AppState::new(
        policy,
        limiter,
        Arc::clone(&identity),
        client,
        field_cipher,
        profile,
    )
    .with_ip_country_header(config.ip_country_header.clone())
    .with_os_age_signals(config.os_age_signals_enabled);
    let sign_in = sign_in_service(&config, identity, Arc::clone(&state.field_cipher), clock)?;
    let state = state.with_sign_in(Arc::new(sign_in));

    let app = api::app(state, &config).into_make_service_with_connect_info::<SocketAddr>();
    let listener = TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("failed to bind {}", config.bind_addr))?;
    tracing::info!(addr = %config.bind_addr, "listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    tracing::info!("shut down cleanly");
    Ok(())
}

/// The worker process role: job lanes plus periodic platform maintenance. Job handlers are
/// registered here as their modules arrive.
async fn run_worker(pool: sqlx::PgPool) {
    let clock: SharedClock = Arc::new(SystemClock);
    let queue = Arc::new(JobQueue::new(pool.clone(), Arc::clone(&clock)));
    let idempotency = Arc::new(IdempotencyStore::new(
        pool.clone(),
        Arc::clone(&clock),
        IdempotencySettings::default(),
    ));
    // Subscribers register here as modules start publishing events.
    let relay = Arc::new(OutboxRelay::new(pool, clock));
    let worker = Worker::new(queue, WorkerSettings::default())
        .periodic(
            "idempotency.purge",
            std::time::Duration::from_secs(3600),
            Arc::new(IdempotencyPurge(idempotency)),
        )
        .periodic(
            "outbox.relay",
            std::time::Duration::from_secs(1),
            Arc::new(RelayTask(relay)),
        );
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let signal = tokio::spawn(async move {
        shutdown_signal().await;
        let _ = stop.send(true);
    });
    worker.run(stopped).await;
    signal.abort();
}

/// Deletes Idempotency-Key rows past their 24 h retention.
struct IdempotencyPurge(Arc<IdempotencyStore>);

impl PeriodicTask for IdempotencyPurge {
    fn run(&self) -> PeriodicFuture {
        let store = Arc::clone(&self.0);
        Box::pin(async move {
            let purged = store.purge_expired().await.map_err(|e| e.to_string())?;
            tracing::info!(event = "idempotency.purged", purged);
            Ok(())
        })
    }
}

/// Apple (T-19) and Google (T-20) sign-in: ID-token verifiers with cached JWKS, plus the Apple
/// code exchange when the team key is configured.
fn sign_in_service(
    config: &Config,
    identity: Arc<IdentityService>,
    cipher: Arc<FieldCipher>,
    clock: SharedClock,
) -> anyhow::Result<ProviderSignIn> {
    let http = identity::oidc::provider_http_client()?;
    let clock_skew = time::Duration::seconds(
        i64::try_from(config.auth_clock_skew.as_secs()).context("clock skew out of range")?,
    );
    let settings = |client_ids: &[String]| VerifierSettings {
        client_ids: client_ids.to_vec(),
        clock_skew,
        jwks: JwksSettings::default(),
    };
    let apple = IdTokenVerifier::new(
        ProviderSpec::apple(),
        settings(&config.apple_client_ids),
        http.clone(),
        Arc::clone(&clock),
    )?;
    if config.apple_client_ids.is_empty() {
        tracing::warn!("API_APPLE_CLIENT_IDS is empty: Apple sign-in answers 503");
    }
    let google = IdTokenVerifier::new(
        ProviderSpec::google(),
        settings(&config.google_client_ids),
        http.clone(),
        Arc::clone(&clock),
    )?;
    if config.google_client_ids.is_empty() {
        tracing::warn!("API_GOOGLE_CLIENT_IDS is empty: Google sign-in answers 503");
    }
    let apple_tokens: Arc<dyn AppleTokenClient> = match &config.apple_signing {
        Some(signing) => {
            let signer = AppleClientSecretSigner::new(
                &signing.team_id,
                &signing.key_id,
                signing.private_key.expose(),
            )
            .context("invalid API_APPLE_PRIVATE_KEY")?;
            Arc::new(AppleHttpClient::new(http, signer, clock))
        }
        None => {
            tracing::warn!("Apple team key not configured: authorization codes are not exchanged");
            Arc::new(UnconfiguredAppleClient)
        }
    };
    Ok(ProviderSignIn::new(identity, apple_tokens, cipher)
        .with_verifier(apple)
        .with_verifier(google))
}

/// Resolves on Ctrl+C or SIGTERM (container stop). In-flight requests are drained;
/// each is bounded by the request timeout, so shutdown is bounded too.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %err, "failed to listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => {
                tracing::error!(error = %err, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received, draining connections");
}
