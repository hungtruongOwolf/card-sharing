use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use api::client_config::ClientConfig;
use api::config::Config;
use api::state::AppState;
use identity::{IdentityService, SessionPolicy};
use platform::clock::{SharedClock, SystemClock};
use platform::rate_limit::{RateLimitConfig, RateLimiter};
use policy::PolicyService;
use tokio::net::TcpListener;

/// `api` serves HTTP; `api migrate` applies database migrations and exits (run it from the
/// deploy pipeline before rolling out a new version).
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
        Some(other) => {
            anyhow::bail!("unknown command {other:?} (expected no argument or `migrate`)")
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
        clock,
        SessionPolicy::default(),
    ));

    let client = ClientConfig::from_path(&config.client_config_path)
        .context("failed to load client config")?;
    let state = AppState::new(policy, limiter, identity, client)
        .with_ip_country_header(config.ip_country_header.clone());

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
