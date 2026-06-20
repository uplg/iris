//! One-shot backfill: embed every catalogue item that lacks a current-model
//! content embedding, writing the vectors back into `catalog_items`.
//!
//! This is the bootstrap pass — at steady state the freshness scheduler embeds
//! each new item as it enters the window. Run after deploying the reco engine,
//! or after a model swap (which invalidates the stored `embedding_model` stamp):
//!
//! ```text
//! IRIS_CONFIG=config/config.toml cargo run -p iris-api --bin gen-embeddings
//! ```
//!
//! The model table (≈32 MB i8) lives only here; the request path never loads it.

use std::collections::HashMap;

use anyhow::{Context, Result};

use iris_api::tmdb::{TmdbClient, TmdbKind};
use iris_reco::Embedder;
use iris_reco::text::{self, ItemText};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let config_path =
        std::env::var("IRIS_CONFIG").unwrap_or_else(|_| "config/config.toml".to_owned());
    let cfg = iris_config::AppConfig::load(&config_path)
        .with_context(|| format!("loading config from {config_path}"))?;

    if !cfg.reco.enabled {
        tracing::warn!("reco engine disabled in config; nothing to embed");
        return Ok(());
    }

    let db_path = cfg.storage.data_dir.join("iris.db");
    let pool = iris_db::connect(&db_path)
        .await
        .with_context(|| format!("connecting to db at {}", db_path.display()))?;

    tracing::info!(model = %cfg.reco.model, "loading model2vec embedder");
    let embedder = Embedder::load(&cfg.reco.model)?;

    let genre_names = load_genre_names(&cfg).await;
    tracing::info!(genres = genre_names.len(), "resolved TMDB genre taxonomy");

    let mut total = 0usize;
    loop {
        let batch =
            iris_db::catalog::items_needing_embedding(&pool, embedder.model_id(), cfg.reco.embed_batch)
                .await?;
        if batch.is_empty() {
            break;
        }

        let texts: Vec<String> = batch
            .iter()
            .map(|it| {
                let genres: Vec<String> = it
                    .genres
                    .iter()
                    .filter_map(|id| genre_names.get(id).cloned())
                    .collect();
                text::build(&ItemText {
                    title: &it.title,
                    overview: it.overview.as_deref(),
                    genres: &genres,
                    cast: &[],
                    keywords: &[],
                })
            })
            .collect();

        let vectors = embedder.embed(&texts);
        for (it, vector) in batch.iter().zip(&vectors) {
            iris_db::catalog::set_embedding(&pool, it.id, vector, embedder.model_id()).await?;
        }
        total += batch.len();
        tracing::info!(total, "embedded items");
    }

    tracing::info!(total, model = %embedder.model_id(), dim = embedder.dim(), "backfill complete");
    Ok(())
}

/// Build the TMDB genre `id → name` map from the cached taxonomy (movie + tv).
/// Empty when no TMDB key is configured — items then embed without genre words,
/// which is degraded but not fatal.
async fn load_genre_names(cfg: &iris_config::AppConfig) -> HashMap<i64, String> {
    let mut names = HashMap::new();
    let Some(tmdb) = &cfg.tmdb else {
        tracing::warn!("no [tmdb] config; embedding without genre names");
        return names;
    };
    let Ok(client) = TmdbClient::new(tmdb.api_key.clone()) else {
        return names;
    };
    for kind in [TmdbKind::Movie, TmdbKind::Tv] {
        for genre in client.genre_list(kind).await {
            names.entry(i64::from(genre.id)).or_insert(genre.name);
        }
    }
    names
}
