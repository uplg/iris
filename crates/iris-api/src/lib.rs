pub mod anilist;
pub mod app;
pub mod client_version;
pub mod collection_assign;
pub mod collections_scheduler;
pub mod error;
pub mod freshness_scheduler;
pub mod middleware;
pub mod observability;
pub mod presence;
pub mod ranking;
pub mod rate_limit;
pub mod reco;
pub mod routes;
pub mod seed_stats;
pub mod state;
pub mod tmdb;
pub mod tmdb_backfill;
pub mod tmdb_resolve;

use std::path::{Path, PathBuf};

/// Build the remux cache manager and spawn its background size-cap evictor.
/// The 100 GB hard ceiling here is a safety net independent of disk
/// pressure; the disk GC also calls into the remuxer first when the
/// overall budget is exceeded, evicting oldest-played caches before
/// torrents.
fn setup_remuxer(data_dir: &Path) -> anyhow::Result<(iris_media::RemuxManager, PathBuf)> {
    use anyhow::Context;
    let remux_dir = data_dir.join("remux");
    std::fs::create_dir_all(&remux_dir).context("creating remux dir")?;
    let remuxer = iris_media::RemuxManager::new(remux_dir.clone());
    let evictor = remuxer.clone();
    let cap_bytes: u64 = 100 * 1_073_741_824;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await; // skip immediate boot tick
        loop {
            ticker.tick().await;
            let (count, _) = evictor.evict_to(cap_bytes).await;
            if count > 0 {
                tracing::info!(count, "remuxer cache eviction pass complete");
            }
        }
    });
    Ok((remuxer, remux_dir))
}

/// Wire the disk GC to both the torrent engine and the remux cache so
/// it can shed the regenerable bytes first under pressure. Extracted
/// from `run` to keep that function under the clippy line-count budget.
fn setup_gc(
    cfg: &iris_config::AppConfig,
    engine: &std::sync::Arc<iris_torrent::Engine>,
    pool: &iris_db::SqlitePool,
    remuxer: &iris_media::RemuxManager,
    remux_dir: PathBuf,
) -> iris_torrent::Gc {
    // Derived-cache hook: when total disk usage tops the threshold the
    // GC asks this closure to shrink the remux dir to `target` bytes
    // (oldest-played first) before any torrent gets evicted. Returns
    // bytes actually freed.
    let derived = {
        let remuxer = remuxer.clone();
        iris_torrent::DerivedCache {
            dir: remux_dir,
            trim_to: std::sync::Arc::new(move |target: u64| {
                let remuxer = remuxer.clone();
                Box::pin(async move {
                    let (_count, freed) = remuxer.evict_to(target).await;
                    freed
                })
            }),
        }
    };

    // Wipe matching remux cache dirs (one per file index, named
    // `{infohash}_{idx}`) after a torrent is evicted, so derived state
    // doesn't outlive its source.
    let on_evict = {
        let remuxer = remuxer.clone();
        move |infohash: &str| {
            let remuxer = remuxer.clone();
            let h = infohash.to_string();
            tokio::spawn(async move {
                let cache_dir = remuxer.base_dir().to_path_buf();
                let prefix = format!("{h}_");
                if let Ok(mut rd) = tokio::fs::read_dir(&cache_dir).await {
                    while let Ok(Some(e)) = rd.next_entry().await {
                        if let Some(name) = e.file_name().to_str() {
                            if name.starts_with(&prefix) {
                                let _ = tokio::fs::remove_file(e.path()).await;
                            }
                        }
                    }
                }
            });
        }
    };

    iris_torrent::Gc::new(
        engine.clone(),
        pool.clone(),
        iris_torrent::GcConfig {
            max_storage_bytes: cfg.storage.max_storage_gb.saturating_mul(1_073_741_824),
            cleanup_threshold_pct: cfg.storage.cleanup_threshold_pct,
            cleanup_target_pct: cfg.storage.cleanup_target_pct,
            interval: std::time::Duration::from_secs(15 * 60),
            active_window: std::time::Duration::from_secs(60 * 60),
        },
        cfg.storage.download_dir.clone(),
        Some(derived),
        on_evict,
    )
}

/// Background tasks that depend on the live `AppState`. Extracted from
/// `run` to keep that function under the clippy line-count budget.
fn spawn_background_jobs(
    app_state: &state::AppState,
    pool: iris_db::SqlitePool,
    provider_registry: iris_providers::ProviderRegistry,
) {
    // Notify scheduler walks TV `collections` every 4 h, queries
    // the indexer with each collection's display name, SCENE-parses
    // every hit, and pre-caches new (S, E) entries in
    // `available_episodes` so the user's "Prepare" / "Play next"
    // clicks go through the fast path. No TMDB call. Never ingests
    // on its own.
    collections_scheduler::spawn(pool.clone(), provider_registry);

    // Discovery freshness scheduler: the tracker RSS rolling window. Polls
    // each provider's latest-releases feed one (provider × kind) slice per
    // tick, correlates each fresh release to TMDB, and upserts grabbable
    // candidates into `catalog_items` (availability='available'), GCing the
    // window each cycle. Tracker-first — TMDB is correlation only. Needs TMDB
    // configured (for poster/genre enrichment) and at least one provider.
    if let Some(tmdb) = app_state.tmdb() {
        freshness_scheduler::spawn(
            pool.clone(),
            tmdb.clone(),
            app_state.providers().clone(),
            app_state.cfg().discovery.clone(),
        );
    }

    // Lifetime-upload reconciler — every 30 s, merge librqbit's session
    // upload counters into `torrents.uploaded_bytes_total` so the value
    // survives restarts and GC evictions.
    seed_stats::spawn(pool.clone(), app_state.engine().clone());

    // One-shot TMDB id migration: legacy torrents were ingested with
    // the indexer's (often wrong) tmdb_id; this sweep re-resolves each
    // one from its SCENE-cleaned name and re-runs runtime verification
    // against the corrected id. New ingests already go through the
    // override path in `routes::torrents::ingest`, so this only needs
    // to run once at boot.
    tmdb_backfill::spawn(app_state.clone());

    // One-shot at boot: complete episodes left stuck in-progress (e.g. "97 %")
    // because the viewer skipped the credits and jumped ahead before the
    // per-heartbeat "moved on ⇒ previous done" hook existed. Sweeps the whole
    // backlog, including episodes several behind the current frontier (the
    // live hook only reaches the immediate predecessor). Idempotent — a no-op
    // once converged.
    {
        let db = app_state.db().clone();
        tokio::spawn(async move {
            match iris_db::playback::backfill_complete_superseded_episodes(&db).await {
                Ok(n) if n > 0 => {
                    tracing::info!(completed = n, "backfilled superseded-episode completions");
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "superseded-episode backfill failed"),
            }
        });
    }

    // Collection assignment backfill — attaches a `collections` row to
    // every existing torrent that lacks one. Runs at boot AND every
    // 5 min after that — the engine's snapshot list isn't fully
    // populated for several seconds (sometimes minutes for old
    // torrents whose metadata is still being fetched), and the boot-
    // only path missed those. The periodic re-run is a no-op when
    // nothing's left to assign.
    let bf_pool = pool;
    let bf_tmdb = app_state.tmdb().cloned();
    let bf_anilist = app_state.anilist().cloned();
    let bf_providers = app_state.providers().clone();
    let bf_engine = app_state.engine().clone();
    tokio::spawn(async move {
        // First pass: short delay so the engine has at least started
        // loading. The retry loop catches the slow stragglers.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        collection_assign::run_backfill(
            &bf_pool,
            collection_assign::EnrichDeps {
                tmdb: bf_tmdb.as_ref(),
                anilist: bf_anilist.as_ref(),
                providers: Some(&bf_providers),
            },
            &bf_engine,
        )
        .await;
        let mut ticker =
            tokio::time::interval(std::time::Duration::from_secs(5 * 60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await; // skip the immediate fire
        loop {
            ticker.tick().await;
            collection_assign::run_backfill(
            &bf_pool,
            collection_assign::EnrichDeps {
                tmdb: bf_tmdb.as_ref(),
                anilist: bf_anilist.as_ref(),
                providers: Some(&bf_providers),
            },
            &bf_engine,
        )
        .await;
        }
    });
}

pub async fn run(config_path: PathBuf, providers_override: Option<PathBuf>) -> anyhow::Result<()> {
    use anyhow::Context;

    let cfg = iris_config::AppConfig::load(&config_path)
        .with_context(|| format!("loading config {}", config_path.display()))?;

    let providers_cfg = cfg
        .load_providers(providers_override.as_deref())
        .context("loading providers config")?;

    let db_path = cfg.storage.data_dir.join("iris.db");
    let pool = iris_db::connect(&db_path)
        .await
        .with_context(|| format!("connecting to db at {}", db_path.display()))?;
    iris_db::migrate::run(&pool).await.context("running migrations")?;

    state::bootstrap_admin_if_configured(&pool, &cfg.auth)
        .await
        .context("bootstrap admin")?;

    let provider_registry = iris_providers::ProviderRegistry::from_entries(&providers_cfg.providers)
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "no providers loaded; continuing with empty registry");
            iris_providers::ProviderRegistry::default()
        });

    let engine = iris_torrent::Engine::new(
        cfg.storage.download_dir.clone(),
        cfg.storage.data_dir.join("librqbit"),
        cfg.storage.torrent_port,
    )
    .await
    .context("starting torrent engine")?;
    tracing::info!(
        download_dir = %cfg.storage.download_dir.display(),
        torrent_port = cfg.storage.torrent_port,
        "torrent engine ready"
    );

    let (remuxer, remux_dir) =
        setup_remuxer(&cfg.storage.data_dir).context("setting up remuxer")?;

    let gc = setup_gc(&cfg, &engine, &pool, &remuxer, remux_dir);
    gc.clone().spawn();
    tracing::info!(
        max_gb = cfg.storage.max_storage_gb,
        threshold = cfg.storage.cleanup_threshold_pct,
        target = cfg.storage.cleanup_target_pct,
        "disk gc loop started"
    );

    let app_state = state::AppState::new(
        cfg.clone(),
        pool.clone(),
        provider_registry.clone(),
        engine,
        remuxer,
        gc,
    );

    spawn_background_jobs(&app_state, pool.clone(), provider_registry);

    let router = app::build_router(app_state);
    let service = app::into_service(router);

    let listener = tokio::net::TcpListener::bind(&cfg.server.bind)
        .await
        .with_context(|| format!("binding to {}", cfg.server.bind))?;
    tracing::info!(addr = %cfg.server.bind, "iris listening");
    axum::serve(listener, axum::ServiceExt::<axum::extract::Request>::into_make_service(service))
        .await
        .context("axum serve")?;
    Ok(())
}
