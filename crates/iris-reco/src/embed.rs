//! `model2vec` static-embedding wrapper.

use anyhow::{Context, Result};
use model2vec_rs::model::StaticModel;

/// A loaded static sentence model — a `token → vector` lookup table (model2vec).
///
/// **Memory model — read before worrying about RAM:**
/// - The struct holds the model's lookup table, loaded **once**. That table is the
///   whole footprint (an English potion ≈ 8–128 MB depending on size/quant; the
///   500k-vocab multilingual one is ~512 MB in f32, ~128 MB in i8). It is **not**
///   the catalogue.
/// - It belongs to the **ingest path only** (freshness scheduler + one-shot
///   backfill). Each item is embedded **once** as it enters the window and the
///   resulting vector (model-dependent dim — 512 floats for potion-retrieval-32M)
///   is persisted as a small BLOB.
/// - The **request hot path never builds an `Embedder` and never embeds.** It
///   ranks over the precomputed vector table (a few MB for the whole catalogue)
///   with plain dot products — microseconds, zero model in RAM.
///
/// Output vectors are L2-normalized, so every cosine downstream is just a dot.
pub struct Embedder {
    model: StaticModel,
    model_id: String,
    dim: usize,
}

impl Embedder {
    /// Load a model by a Hugging Face repo id (downloaded + cached on first use)
    /// or by local path — `model2vec-rs` accepts either for `repo_or_path`.
    ///
    /// # Errors
    /// Fails if the model can't be fetched/parsed or yields a zero-length vector.
    pub fn load(model_id: &str) -> Result<Self> {
        let model = StaticModel::from_pretrained(model_id, None, Some(true), None)
            .with_context(|| format!("loading model2vec model '{model_id}'"))?;
        let dim = model.encode_single("dimension probe").len();
        anyhow::ensure!(
            dim > 0,
            "model '{model_id}' produced a zero-length embedding"
        );
        Ok(Self {
            model,
            model_id: model_id.to_owned(),
            dim,
        })
    }

    /// Embedding dimensionality.
    #[must_use]
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// The model id this embedder was loaded from — stamped alongside stored
    /// vectors so a model swap invalidates them.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Embed a batch of texts; each output row is L2-normalized.
    #[must_use]
    pub fn embed(&self, texts: &[String]) -> Vec<Vec<f32>> {
        self.model.encode(texts)
    }

    /// Embed a single text; output is L2-normalized.
    #[must_use]
    pub fn embed_one(&self, text: &str) -> Vec<f32> {
        self.model.encode_single(text)
    }
}
