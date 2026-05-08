pub mod app;
pub mod error;
pub mod observability;
pub mod routes;
pub mod state;

use std::path::PathBuf;

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
    )
    .await
    .context("starting torrent engine")?;
    tracing::info!(
        download_dir = %cfg.storage.download_dir.display(),
        "torrent engine ready"
    );

    let hls_dir = cfg.storage.data_dir.join("hls");
    std::fs::create_dir_all(&hls_dir).context("creating hls dir")?;
    let hls = iris_media::HlsManager::new(hls_dir);

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
            let hls = hls.clone();
            move |infohash| {
                let hls = hls.clone();
                let h = infohash.to_string();
                tokio::spawn(async move { hls.cleanup_for_torrent(&h).await });
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

    let app_state = state::AppState::new(cfg.clone(), pool, provider_registry, engine, hls, gc);
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
