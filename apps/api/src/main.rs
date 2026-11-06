use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use api::client_config::ClientConfig;
use api::config::{AttestationConfig, Config, MailConfig, MailTransport};
use api::state::{AppState, ProfileSettings};
use attestation::apple::AppAttestVerifier;
use attestation::play::PlayIntegrityVerifier;
use identity::apple::{
    AppleClientSecretSigner, AppleHttpClient, AppleTokenClient, UnconfiguredAppleClient,
};
use identity::mail::{Mailer, PostmarkMailer, SmtpMailer, UnconfiguredMailer};
use identity::oidc::{IdTokenVerifier, JwksSettings, ProviderSpec, VerifierSettings};
use identity::{AttestationService, IdentityService, ProfileRules, ProviderSignIn, SessionPolicy};
use identity::{MAGIC_LINK_JOB, MagicLinkJob, MagicLinkTemplate};
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
/// (ADR-0006) until SIGTERM; `api seed` writes local demo data (development and test only);
/// `api replay-erasures` re-erases every erased account after a backup restore (T-152).
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
            return run_worker(&config, pool).await;
        }
        Some("seed") => {
            return run_seed(&config, pool).await;
        }
        Some("replay-erasures") => {
            // Restore drill (T-152): after restoring a backup, erase every account that was erased
            // before, so their data does not come back.
            let state = build_state(&config, pool)?;
            let replayed = state
                .deletion()
                .replay_erasures()
                .await
                .context("replaying erasures failed")?;
            println!("Replayed {replayed} erasures.");
            return Ok(());
        }
        Some(other) => {
            anyhow::bail!(
                "unknown command {other:?} (expected no argument, `migrate`, `worker`, `seed` or `replay-erasures`)"
            )
        }
    }

    let state = build_state(&config, pool)?;
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

/// The HTTP application state from config (shared by the server and `api seed`).
fn build_state(config: &Config, pool: sqlx::PgPool) -> anyhow::Result<AppState> {
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
    let social = social::SocialSettings::from_path(&config.social_config_path)
        .context("failed to load social config")?;
    let board_settings = boards::BoardSettings::from_path(&config.boards_config_path)
        .context("failed to load boards config")?;
    let sharing_settings = sharing::SharingSettings::from_path(&config.sharing_config_path)
        .context("failed to load sharing config")?;
    let privacy_settings = privacy::PrivacySettings::from_path(&config.privacy_config_path)
        .context("failed to load privacy config")?;
    let state = AppState::new(
        policy,
        limiter,
        Arc::clone(&identity),
        client,
        field_cipher,
        profile,
    )
    .with_ip_country_header(config.ip_country_header.clone())
    .with_os_age_signals(config.os_age_signals_enabled)
    .with_social_settings(social)
    .with_board_settings(board_settings)
    .with_sharing_settings(sharing_settings)
    .with_privacy_settings(privacy_settings)
    .with_attestation(
        attestation_service(&config.attestation)?,
        config.attestation.mode,
    );
    let state = match media_store(config, &clock)? {
        Some(store) => state.with_media_store(store),
        None => state,
    };
    let state = match &config.media_signing_key {
        Some(key) => state.with_media_signer(Arc::new(
            media::UrlSigner::from_base64(key.expose(), &config.media_base_url)
                .context("invalid API_MEDIA_SIGNING_KEY")?,
        )),
        None => {
            tracing::warn!("API_MEDIA_SIGNING_KEY is unset: routes that return media answer 503");
            state
        }
    };
    let sign_in = sign_in_service(config, identity, Arc::clone(&state.field_cipher), clock)?;
    let state = state.with_sign_in(Arc::new(sign_in));

    Ok(state)
}

/// `api seed`: local demo users, friendships, cards and a shared board (src/seed.rs). Refused
/// unless `API_ENV` is development or test; creates the S3 buckets when S3 storage is configured.
async fn run_seed(config: &Config, pool: sqlx::PgPool) -> anyhow::Result<()> {
    use api::config::AppEnv;
    if !matches!(
        config.content_safety.env,
        AppEnv::Development | AppEnv::Test
    ) {
        anyhow::bail!(
            "`api seed` only runs with API_ENV=development or test (now {})",
            config.content_safety.env.as_str()
        );
    }
    if let Some(s3) = &config.media_s3 {
        let store = media::S3ObjectStore::new(s3.clone(), Arc::new(SystemClock))
            .context("invalid S3 media storage settings")?;
        for bucket in [
            media::Bucket::Quarantine,
            media::Bucket::Clean,
            media::Bucket::LegalHold,
        ] {
            store
                .create_bucket(bucket)
                .await
                .with_context(|| format!("could not create the {} bucket", bucket.as_str()))?;
        }
    }
    let state = build_state(config, pool)?;
    let router = api::app(state.clone(), config);
    let gates = media::ProcessSettings {
        gates: api::workers::content_gates(&config.content_safety),
        ..media::ProcessSettings::default()
    };
    let report = api::seed::run(&router, &state, &gates).await?;
    println!(
        "Seeded {} users, {} new cards, shared board {}.",
        report.users.len(),
        report.cards_created,
        report.board_id
    );
    println!("Local development sessions (never share these; they expire like real ones):");
    for user in &report.users {
        println!(
            "  @{:<11} id={} minor={}
    access_token={}
    refresh_token={}",
            user.persona.handle,
            user.id,
            user.persona.age < 18,
            user.session.access_token.expose(),
            user.session.refresh_token.expose()
        );
    }
    Ok(())
}

/// Object storage for media from config: S3-compatible (R2) or the local filesystem; `None` =
/// uploads answer 503 (fail closed).
fn media_store(
    config: &Config,
    clock: &SharedClock,
) -> anyhow::Result<Option<Arc<dyn media::ObjectStore>>> {
    if let Some(s3) = &config.media_s3 {
        tracing::info!(region = %s3.region, "S3-compatible media storage");
        let store = media::S3ObjectStore::new(s3.clone(), Arc::clone(clock))
            .context("invalid S3 media storage settings")?;
        return Ok(Some(Arc::new(store)));
    }
    Ok(match &config.media_storage_dir {
        Some(dir) => {
            tracing::warn!(
                dir = %dir.display(),
                "filesystem media storage (local development and CI only)"
            );
            Some(Arc::new(media::FsObjectStore::new(dir.clone())))
        }
        None => {
            tracing::warn!(
                "no media storage (API_S3_ENDPOINT and API_MEDIA_STORAGE_DIR unset): media uploads answer 503"
            );
            None
        }
    })
}

/// App Attest (Apple's root) and Play Integrity verifiers from config; an unconfigured platform's
/// proofs fail as `unconfigured` (recorded, and refused only in enforce mode).
fn attestation_service(config: &AttestationConfig) -> anyhow::Result<AttestationService> {
    let apple = if config.app_attest_app_ids.is_empty() {
        tracing::warn!(
            "API_APP_ATTEST_APP_IDS is empty: iOS attestation proofs fail as unconfigured"
        );
        None
    } else {
        Some(
            AppAttestVerifier::apple(
                config.app_attest_app_ids.clone(),
                config.app_attest_allow_development,
            )
            .context("invalid App Attest root")?,
        )
    };
    let play = match &config.play_integrity {
        None => {
            tracing::warn!(
                "Play Integrity keys not configured: Android proofs fail as unconfigured"
            );
            None
        }
        Some(play) => Some(
            PlayIntegrityVerifier::from_base64(
                play.package_names.clone(),
                play.decryption_key.expose(),
                &play.verification_key,
            )
            .context("invalid Play Integrity keys")?,
        ),
    };
    tracing::info!(mode = ?config.mode, "device attestation configured");
    Ok(AttestationService::new(apple, play))
}

/// The worker process role: job lanes plus periodic platform maintenance. Job handlers are
/// registered here as their modules arrive.
async fn run_worker(config: &Config, pool: sqlx::PgPool) -> anyhow::Result<()> {
    let clock: SharedClock = Arc::new(SystemClock);
    let cipher = Arc::new(
        FieldCipher::from_base64(1, config.data_encryption_key.expose())
            .context("invalid API_DATA_ENCRYPTION_KEY")?,
    );
    // Redis lets suspensions drop cached sessions at once (not only after the cache TTL).
    let redis = platform::cache::connect_lazy(&config.redis_url).context("invalid REDIS_URL")?;
    let identity = Arc::new(IdentityService::new(
        pool.clone(),
        Some(redis),
        Arc::clone(&clock),
        SessionPolicy::default(),
    ));
    let push_tokens = Arc::new(notifications::PushTokens::new(
        pool.clone(),
        Arc::clone(&clock),
        Arc::clone(&cipher),
    ));
    let magic_links = MagicLinkJob {
        identity: Arc::clone(&identity),
        cipher: Arc::clone(&cipher),
        mailer: mailer(&config.mail)?,
        template: MagicLinkTemplate {
            link_url: config.mail.magic_link_url.clone(),
        },
    };
    let queue = Arc::new(JobQueue::new(pool.clone(), Arc::clone(&clock)));
    let idempotency = Arc::new(IdempotencyStore::new(
        pool.clone(),
        Arc::clone(&clock),
        IdempotencySettings::default(),
    ));
    let social = social::SocialSettings::from_path(&config.social_config_path)
        .context("failed to load social config")?;
    let janitor = Arc::new(social::SocialJanitor::new(
        pool.clone(),
        Arc::clone(&clock),
        social.retention_days,
    ));
    let board_janitor = Arc::new(boards::BoardJanitor::new(pool.clone(), Arc::clone(&clock)));
    let block_resolver = Arc::new(boards::BlockResolver::new(pool.clone(), Arc::clone(&clock)));
    let board_changes = Arc::new(boards::ChangeNotifier::new(
        pool.clone(),
        Arc::clone(&clock),
    ));
    let media = Arc::new(media::MediaService::new(
        pool.clone(),
        Arc::clone(&clock),
        media_store(config, &clock)?,
    ));
    // Public snapshots (T-130) read boards as their owner: the boards service needs the age
    // service for the minor rule.
    let policy = Arc::new(
        PolicyService::from_path(&config.policy_config_path)
            .context("failed to load region policy config")?,
    );
    let age = Arc::new(identity::AgeService::new(
        Arc::clone(&identity),
        policy,
        Arc::clone(&cipher),
        config.os_age_signals_enabled,
    ));
    let board_service = Arc::new(boards::BoardService::new(
        pool.clone(),
        Arc::clone(&clock),
        boards::BoardSettings::from_path(&config.boards_config_path)
            .context("failed to load boards config")?,
        age,
    ));
    let snapshots = Arc::new(sharing::SnapshotBuilder::new(
        pool.clone(),
        Arc::clone(&clock),
        Arc::clone(&board_service),
        Arc::clone(&media),
    ));
    let privacy_settings = privacy::PrivacySettings::from_path(&config.privacy_config_path)
        .context("failed to load privacy config")?;
    let registry = api::privacy::registry_from(
        Arc::clone(&identity),
        Arc::clone(&cipher),
        Arc::clone(&media),
        board_service,
    );
    let exports = Arc::new(privacy::ExportService::new(
        pool.clone(),
        Arc::clone(&clock),
        registry.clone(),
        Arc::clone(&media),
        privacy_settings.clone(),
    ));
    let deletion = Arc::new(
        privacy::DeletionService::new(Arc::clone(&identity), registry, privacy_settings)
            .with_sign_in(Arc::new(sign_in_service(
                config,
                Arc::clone(&identity),
                Arc::clone(&cipher),
                Arc::clone(&clock),
            )?)),
    );
    let cdn: Arc<dyn sharing::CdnPurger> = match config.cdn_purger {
        api::config::SafetyAdapter::Fake
            if config.content_safety.env != api::config::AppEnv::Production =>
        {
            tracing::warn!("API_CDN_PURGER=fake: CDN purges are recorded, not sent");
            Arc::new(sharing::FakeCdnPurger::default())
        }
        _ => {
            tracing::warn!("no CDN purger configured: revoked links are not purged from a CDN");
            Arc::new(sharing::NoCdn)
        }
    };
    let purger = Arc::new(sharing::Purger::new(
        pool.clone(),
        Arc::clone(&clock),
        Arc::clone(&media),
        cdn,
    ));
    // Subscribers register here as modules start publishing events.
    let card_service = Arc::new(cards::CardService::new(pool.clone(), Arc::clone(&clock)));
    let card_jobs = cards::EvaluateJob {
        cards: Arc::clone(&card_service),
        media: Arc::clone(&media),
    };
    let purge_jobs = cards::PurgeJob {
        cards: Arc::clone(&card_service),
        media: Arc::clone(&media),
    };
    let notifier = Arc::new(notifications::Notifier::new(
        pool.clone(),
        Arc::clone(&clock),
        Arc::clone(&push_tokens),
        push_sender(config),
        notifications::NotifySettings::from_path(&config.notifications_config_path)
            .context("failed to load notifications config")?,
    ));
    let activity = Arc::new(notifications::Activity::new(
        pool.clone(),
        Arc::clone(&clock),
    ));
    let relay = Arc::new(
        OutboxRelay::new(pool, clock)
            .subscribe(Arc::new(notifications::PushOnEvents))
            .subscribe(Arc::new(notifications::ActivityOnEvents))
            .subscribe(Arc::new(api::workers::CsamSuspension))
            .subscribe(Arc::new(cards::EvaluateOnEvents))
            .subscribe(Arc::new(cards::PurgeOnDelete))
            .subscribe(Arc::new(boards::ResolveBlocksOnEvents))
            .subscribe(Arc::new(sharing::SnapshotOnEvents))
            .subscribe(Arc::new(sharing::PurgeOnBoardDeleted)),
    );
    let slo_report = api::workers::SloReport {
        cards: card_service,
        media: Arc::clone(&media),
        queue: Arc::clone(&queue),
        notifier: Arc::clone(&notifier),
        tokens: Arc::clone(&push_tokens),
    };
    let worker = Worker::new(queue, WorkerSettings::default())
        .handle(MAGIC_LINK_JOB, Arc::new(magic_links))
        .handle(
            media::PROCESS_JOB,
            Arc::new(media::ProcessMediaJob {
                media: Arc::clone(&media),
                settings: media::ProcessSettings {
                    gates: api::workers::content_gates(&config.content_safety),
                    ..media::ProcessSettings::default()
                },
            }),
        )
        .handle(cards::EVALUATE_JOB, Arc::new(card_jobs))
        .handle(cards::PURGE_JOB, Arc::new(purge_jobs))
        .handle(
            boards::RESOLVE_BLOCK_JOB,
            Arc::new(boards::ResolveBlockJob(block_resolver)),
        )
        .handle(
            boards::CHANGED_JOB,
            Arc::new(boards::BoardChangedJob(board_changes)),
        )
        .handle(
            sharing::SNAPSHOT_JOB,
            Arc::new(sharing::SnapshotJob(snapshots)),
        )
        .handle(
            privacy::HIDE_JOB,
            Arc::new(privacy::HideJob(Arc::clone(&deletion))),
        )
        .handle(
            sharing::PURGE_JOB,
            Arc::new(sharing::PurgeJob(Arc::clone(&purger))),
        )
        .handle(
            notifications::ACTIVITY_JOB,
            Arc::new(notifications::ActivityJob(Arc::clone(&activity))),
        )
        .handle(
            notifications::NOTIFY_JOB,
            Arc::new(notifications::NotifyJob(Arc::clone(&notifier))),
        )
        .handle(
            api::workers::SUSPEND_ACCOUNT_JOB,
            Arc::new(api::workers::SuspendAccountJob(Arc::clone(&identity))),
        )
        .periodic(
            "metrics.slo_report",
            api::workers::SloReport::INTERVAL,
            Arc::new(slo_report),
        )
        .periodic(
            "media.scan_backlog",
            api::workers::ScanBacklog::INTERVAL,
            Arc::new(api::workers::ScanBacklog(media)),
        )
        .periodic(
            "identity.magic_links.purge",
            std::time::Duration::from_secs(3600),
            Arc::new(MagicLinkPurge(identity)),
        )
        .periodic(
            "notifications.push_tokens.prune",
            std::time::Duration::from_secs(600),
            Arc::new(PushTokenPrune(push_tokens)),
        )
        .periodic(
            "notifications.activity.purge",
            std::time::Duration::from_secs(3600),
            Arc::new(ActivityPurge(activity)),
        )
        .periodic(
            "notifications.push_log.purge",
            std::time::Duration::from_secs(3600),
            Arc::new(PushLogPurge(notifier)),
        )
        .periodic(
            "idempotency.purge",
            std::time::Duration::from_secs(3600),
            Arc::new(IdempotencyPurge(idempotency)),
        )
        .periodic(
            "privacy.export_due",
            privacy::EXPORT_INTERVAL,
            Arc::new(privacy::ExportDue(exports)),
        )
        .periodic(
            "privacy.erase_due",
            privacy::ERASE_INTERVAL,
            Arc::new(privacy::EraseDue(deletion)),
        )
        .periodic(
            "sharing.purge_sweep",
            sharing::SWEEP_INTERVAL,
            Arc::new(sharing::PurgeSweep(purger)),
        )
        .periodic(
            "boards.expire_invitations",
            std::time::Duration::from_secs(600),
            Arc::new(BoardHousekeeping(board_janitor)),
        )
        .periodic(
            "social.expire_requests",
            std::time::Duration::from_secs(600),
            Arc::new(SocialHousekeeping(janitor)),
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
    Ok(())
}

/// The mail adapter from config (T-22).
fn mailer(config: &MailConfig) -> anyhow::Result<Arc<dyn Mailer>> {
    Ok(match &config.transport {
        MailTransport::None => {
            tracing::warn!("API_MAIL_TRANSPORT=none: magic-link emails are not sent");
            Arc::new(UnconfiguredMailer)
        }
        MailTransport::Smtp { host, port } => {
            tracing::warn!(%host, port, "plain SMTP mail transport (local development only)");
            Arc::new(SmtpMailer::new(host.clone(), *port, config.from.clone()))
        }
        MailTransport::Postmark { server_token } => Arc::new(PostmarkMailer::new(
            identity::oidc::provider_http_client()?,
            server_token.clone(),
            config.from.clone(),
        )),
    })
}

/// Deletes magic links older than 24 hours.
struct MagicLinkPurge(Arc<IdentityService>);

impl PeriodicTask for MagicLinkPurge {
    fn run(&self) -> PeriodicFuture {
        let identity = Arc::clone(&self.0);
        Box::pin(async move {
            let purged = identity
                .purge_magic_links()
                .await
                .map_err(|e| e.to_string())?;
            tracing::info!(event = "identity.magic_links.purged", purged);
            Ok(())
        })
    }
}

/// Expires friend requests past 14 days (AC-5.4) and purges old social rows.
struct SocialHousekeeping(Arc<social::SocialJanitor>);

impl PeriodicTask for SocialHousekeeping {
    fn run(&self) -> PeriodicFuture {
        let janitor = Arc::clone(&self.0);
        Box::pin(async move {
            let report = janitor.run_once().await.map_err(|e| e.to_string())?;
            tracing::info!(
                event = "social.housekeeping",
                expired = report.expired,
                purged_requests = report.purged_requests,
                purged_invites = report.purged_invites
            );
            Ok(())
        })
    }
}

/// Expires board invitations past 7 days (AC-19.8), purges ended ones and deleted boards.
struct BoardHousekeeping(Arc<boards::BoardJanitor>);

impl PeriodicTask for BoardHousekeeping {
    fn run(&self) -> PeriodicFuture {
        let janitor = Arc::clone(&self.0);
        Box::pin(async move {
            let report = janitor.run_once().await.map_err(|e| e.to_string())?;
            tracing::info!(
                event = "boards.housekeeping",
                expired_invitations = report.expired_invitations,
                purged_invitations = report.purged_invitations,
                purged_boards = report.purged_boards
            );
            Ok(())
        })
    }
}

/// Deletes push tokens that were invalidated by a provider or whose session ended (logout,
/// expiry, suspension, deletion). Senders already skip them; this keeps the table minimal.
struct PushTokenPrune(Arc<notifications::PushTokens>);

impl PeriodicTask for PushTokenPrune {
    fn run(&self) -> PeriodicFuture {
        let tokens = Arc::clone(&self.0);
        Box::pin(async move {
            let pruned = tokens.prune().await.map_err(|e| e.to_string())?;
            tracing::info!(event = "notifications.push_tokens.pruned", pruned);
            Ok(())
        })
    }
}

/// Deletes activity items older than 90 days.
struct ActivityPurge(Arc<notifications::Activity>);

impl PeriodicTask for ActivityPurge {
    fn run(&self) -> PeriodicFuture {
        let activity = Arc::clone(&self.0);
        Box::pin(async move {
            let purged = activity.purge().await.map_err(|e| e.to_string())?;
            tracing::info!(event = "notifications.activity.purged", purged);
            Ok(())
        })
    }
}

/// Deletes push decisions older than 7 days (they only feed the cap and batching windows).
struct PushLogPurge(Arc<notifications::Notifier>);

impl PeriodicTask for PushLogPurge {
    fn run(&self) -> PeriodicFuture {
        let notifier = Arc::clone(&self.0);
        Box::pin(async move {
            let purged = notifier.purge_log().await.map_err(|e| e.to_string())?;
            tracing::info!(event = "notifications.push_log.purged", purged);
            Ok(())
        })
    }
}

/// The push adapter from config: the fake outside production, otherwise none (T-86; real
/// APNs/FCM adapters need credentials).
fn push_sender(config: &Config) -> Arc<dyn notifications::PushSender> {
    match config.push_sender {
        api::config::SafetyAdapter::Fake
            if config.content_safety.env != api::config::AppEnv::Production =>
        {
            tracing::warn!("API_PUSH_SENDER=fake: pushes are recorded, not delivered");
            Arc::new(notifications::FakePushSender::default())
        }
        _ => {
            tracing::warn!("no push adapter configured: pushes are dropped (activity tab only)");
            Arc::new(notifications::UnconfiguredPushSender)
        }
    }
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
