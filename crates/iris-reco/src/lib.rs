//! Content-first recommendation core for Iris.
//!
//! The prod data (see `RECOSYS.md` §0) makes collaborative filtering structurally
//! marginal — only ~0.8% of catalogue candidates have any co-watch signal — so the
//! engine is **content-first**: every item is embedded once with a static
//! `model2vec` sentence model, each user gets a *multi-centroid* taste profile
//! (a household mixes several tastes — kids vs adults — so a single mean vector
//! blurs them), and candidates are scored by cosine to the nearest centroid.
//!
//! This crate is deliberately storage-agnostic: it turns text into vectors and
//! vectors into scores. Persistence (the `content_embedding` BLOB column, profile
//! caching) lives in `iris-db` / `iris-api`.
//!
//! Empirically validated on the prod dump (leave-one-out): nearest-centroid
//! content scoring beats the current linear `fresh_score` ~17× on NDCG@10.

pub mod embed;
pub mod profile;
pub mod score;
pub mod text;

pub use embed::Embedder;

/// Cosine similarity of two **L2-normalized** vectors — i.e. their dot product.
/// The embedder normalizes on output, so callers stay in normalized space and
/// this is the only similarity primitive needed.
///
/// Returns 0.0 on a length mismatch rather than panicking, so a stale-dimension
/// embedding (model changed under us) degrades to "no signal" instead of a crash.
#[must_use]
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// L2-normalize a vector in place. A zero vector is left untouched.
pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// L2-normalized copy of `v`.
#[must_use]
pub fn normalized(v: &[f32]) -> Vec<f32> {
    let mut out = v.to_vec();
    normalize(&mut out);
    out
}
