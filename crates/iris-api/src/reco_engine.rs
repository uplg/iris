//! The content-first reco engine's runtime.
//!
//! Split by where the model lives (see `RECOSYS.md` §5):
//! - **`store`** — `id → vector`, loaded from the DB. The **request path** reads
//!   only this (a few MB; dot products), never the model.
//! - **`embedder`** — the model table, lazily loaded **ingest-side** by the
//!   background embedding loop. A request never touches it.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use iris_db::SqlitePool;
use iris_reco::text::{self, ItemText};
use iris_reco::{Embedder, profile, score};
use tokio::sync::{OnceCell, RwLock};
use uuid::Uuid;

/// k-means seed — fixed so a user's centroids are stable across requests.
const CENTROID_SEED: u64 = 0x5EED_C0DE;
/// Idle wait when there's nothing left to embed.
const EMBED_IDLE: Duration = Duration::from_mins(5);
/// Short breather between non-empty embed batches (yields the box to requests).
const EMBED_BUSY: Duration = Duration::from_secs(2);

pub struct RecoEngine {
    enabled: bool,
    model_id: String,
    centroids_k: usize,
    embed_batch: i64,
    /// `id → L2-normalized embedding`, current model. Empty until the first load
    /// completes (request path then falls back to the legacy scorer).
    store: RwLock<HashMap<Uuid, Vec<f32>>>,
    /// The model table — ingest path only, lazily loaded once.
    embedder: OnceCell<Option<Arc<Embedder>>>,
}

impl RecoEngine {
    pub fn new(cfg: &iris_config::RecoConfig) -> Self {
        Self {
            enabled: cfg.enabled,
            model_id: cfg.model.clone(),
            centroids_k: cfg.centroids.max(1),
            embed_batch: cfg.embed_batch.max(1),
            store: RwLock::new(HashMap::new()),
            embedder: OnceCell::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Rank candidate ids by nearest-centroid cosine to the user's taste profile.
    /// Returns `(id, score)` only for candidates that carry an embedding. An empty
    /// result means cold-start (no positives embedded) — the caller falls back to
    /// the legacy scorer. One read lock for the whole pass.
    pub async fn rank(&self, positives: &[(Uuid, f32)], candidates: &[Uuid]) -> Vec<(Uuid, f32)> {
        let store = self.store.read().await;
        let points: Vec<(Vec<f32>, f32)> = positives
            .iter()
            .filter_map(|(id, w)| store.get(id).map(|v| (v.clone(), *w)))
            .collect();
        if points.is_empty() {
            return Vec::new();
        }
        let centroids = profile::taste_centroids(&points, self.centroids_k, CENTROID_SEED);
        candidates
            .iter()
            .filter_map(|id| store.get(id).map(|v| (*id, score::nearest_centroid(v, &centroids))))
            .collect()
    }

    /// Build the user's taste centroids from their positive catalogue items.
    /// Empty (cold-start) when none of the positives carry an embedding yet.
    pub async fn centroids_for(&self, positives: &[(Uuid, f32)]) -> Vec<Vec<f32>> {
        let store = self.store.read().await;
        let points: Vec<(Vec<f32>, f32)> = positives
            .iter()
            .filter_map(|(id, w)| store.get(id).map(|v| (v.clone(), *w)))
            .collect();
        if points.is_empty() {
            return Vec::new();
        }
        profile::taste_centroids(&points, self.centroids_k, CENTROID_SEED)
    }

    /// Embed texts on the fly with the **already-resident** model (for ranking
    /// fresh TMDB-discover candidates that aren't in the store). Returns `None`
    /// when the model isn't loaded yet — a non-blocking peek, never triggers a
    /// load in the request path — so the caller falls back to a popularity rank.
    pub async fn embed_texts(&self, texts: &[String]) -> Option<Vec<Vec<f32>>> {
        let embedder = self.embedder.get()?.as_ref()?.clone();
        let owned = texts.to_vec();
        tokio::task::spawn_blocking(move || embedder.embed(&owned))
            .await
            .ok()
    }

    /// Reload the in-memory table from the DB (after an embed batch, or at boot).
    pub async fn refresh_store(&self, pool: &SqlitePool) {
        match iris_db::catalog::load_embeddings(pool, &self.model_id).await {
            Ok(rows) => {
                let n = rows.len();
                *self.store.write().await = rows.into_iter().collect();
                tracing::debug!(items = n, "reco store refreshed");
            }
            Err(e) => tracing::warn!(error = %e, "reco store refresh failed"),
        }
    }

    /// Lazily load the model table (ingest side, off the async runtime). `None`
    /// if it can't be loaded — embedding pauses, existing vectors keep serving.
    async fn embedder(&self) -> Option<Arc<Embedder>> {
        self.embedder
            .get_or_init(|| async {
                let model_id = self.model_id.clone();
                match tokio::task::spawn_blocking(move || Embedder::load(&model_id)).await {
                    Ok(Ok(e)) => {
                        tracing::info!(model = %e.model_id(), dim = e.dim(), "reco embedder loaded");
                        Some(Arc::new(e))
                    }
                    Ok(Err(e)) => {
                        tracing::error!(error = %e, "reco embedder load failed");
                        None
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "reco embedder load task panicked");
                        None
                    }
                }
            })
            .await
            .clone()
    }

    /// Background loop: embed catalogue items missing a current-model vector,
    /// refresh the store, idle when caught up. The only place the model is
    /// resident. `genre_names` turns TMDB genre ids into words for the text.
    pub fn spawn_embedding_loop(
        self: Arc<Self>,
        pool: SqlitePool,
        genre_names: HashMap<i64, String>,
    ) {
        if !self.enabled {
            tracing::info!("reco engine disabled; embedding loop not started");
            return;
        }
        tokio::spawn(async move {
            self.refresh_store(&pool).await;
            loop {
                let pending = match iris_db::catalog::items_needing_embedding(
                    &pool,
                    &self.model_id,
                    self.embed_batch,
                )
                .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(error = %e, "reco: items_needing_embedding failed");
                        tokio::time::sleep(EMBED_IDLE).await;
                        continue;
                    }
                };
                if pending.is_empty() {
                    tokio::time::sleep(EMBED_IDLE).await;
                    continue;
                }
                let Some(embedder) = self.embedder().await else {
                    tokio::time::sleep(EMBED_IDLE).await;
                    continue;
                };

                let texts: Vec<String> = pending
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

                let job = embedder.clone();
                let vectors =
                    match tokio::task::spawn_blocking(move || job.embed(&texts)).await {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::error!(error = %e, "reco: embed task panicked");
                            tokio::time::sleep(EMBED_IDLE).await;
                            continue;
                        }
                    };
                for (it, vector) in pending.iter().zip(&vectors) {
                    if let Err(e) =
                        iris_db::catalog::set_embedding(&pool, it.id, vector, &self.model_id).await
                    {
                        tracing::warn!(error = %e, "reco: set_embedding failed");
                    }
                }
                self.refresh_store(&pool).await;
                tracing::info!(count = pending.len(), "reco: embedded batch");
                tokio::time::sleep(EMBED_BUSY).await;
            }
        });
    }
}
