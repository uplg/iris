pub mod app;
pub mod error;
pub mod observability;
pub mod routes;
pub mod state;
pub mod tmdb;

use std::path::{Path, PathBuf};

/// Build the remux cache manager and spawn its background size-cap evictor.
/// Cap at 100 GB (~100 hot movies on a typical 1080p library) — well under
/// the storage budget on a KS-5 SSD. Tick every 15 min.
fn setup_remuxer(data_dir: &Path) -> anyhow::Result<iris_media::RemuxManager> {
    use anyhow::Context;
    let remux_dir = data_dir.join("remux");
    std::fs::create_dir_all(&remux_dir).context("creating remux dir")?;
    let remuxer = iris_media::RemuxManager::new(remux_dir);
    let evictor = remuxer.clone();
    let cap_bytes: u64 = 100 * 1_073_741_824;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await; // skip immediate boot tick
        loop {
            ticker.tick().await;
            let n = evictor.evict_to(cap_bytes).await;
            if n > 0 {
                tracing::info!(count = n, "remuxer cache eviction pass complete");
            }
        }
    });
    Ok(remuxer)
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

    let remuxer = setup_remuxer(&cfg.storage.data_dir).context("setting up remuxer")?;

    let gc = iris_torrent::Gc::new(
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
        {
            let remuxer = remuxer.clone();
            move |infohash| {
                // When a torrent is GC'd off disk, drop the matching remux
                // caches (one per file index). We don't know how many files
                // there are without going back to the DB — wipe by prefix
                // match on the cache file names instead.
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
        },
    );
    gc.clone().spawn();
    tracing::info!(
        max_gb = cfg.storage.max_storage_gb,
        threshold = cfg.storage.cleanup_threshold_pct,
        target = cfg.storage.cleanup_target_pct,
        "disk gc loop started"
    );

    let app_state = state::AppState::new(cfg.clone(), pool, provider_registry, engine, remuxer, gc);
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
