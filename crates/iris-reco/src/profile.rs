//! Per-user multi-centroid taste profile via weighted spherical k-means.
//!
//! Points are L2-normalized embeddings of a user's positives (watched / grabbed),
//! weighted by interaction confidence. `k = 3` centroids was the empirical optimum
//! on the prod dump (k=1 mean blurs a household's distinct tastes, k=5
//! over-segments the sparse data). Deterministic for a given seed.

use rand::RngExt;
use rand::SeedableRng;
use rand::rngs::StdRng;

use crate::{cosine, normalize};

/// Lloyd iterations — convergence is fast on a few dozen unit vectors.
const ITERS: usize = 12;

/// Compute up to `k` taste centroids from weighted, L2-normalized `points`.
///
/// Returns normalized centroids — fewer than `k` only when there are `≤ k`
/// points (each becomes its own centroid). Deterministic for a fixed `seed`
/// (k-means++ init + Lloyd).
#[must_use]
pub fn taste_centroids(points: &[(Vec<f32>, f32)], k: usize, seed: u64) -> Vec<Vec<f32>> {
    if points.is_empty() || k == 0 {
        return Vec::new();
    }
    if points.len() <= k {
        return points.iter().map(|(v, _)| crate::normalized(v)).collect();
    }

    let dim = points[0].0.len();
    let mut rng = StdRng::seed_from_u64(seed);
    let mut centers = kmeanspp_init(points, k, &mut rng);

    for _ in 0..ITERS {
        let mut sums = vec![vec![0f32; dim]; k];
        let mut weights = vec![0f32; k];
        for (v, w) in points {
            let a = assign(v, &centers);
            for (s, x) in sums[a].iter_mut().zip(v) {
                *s += x * w;
            }
            weights[a] += *w;
        }
        // Weighted mean per cluster; the 1/Σw scalar is irrelevant once we
        // L2-normalize, so normalizing the raw weighted sum gives the unit
        // centroid directly. Empty clusters keep their previous center.
        for (center, (sum, w)) in centers.iter_mut().zip(sums.iter_mut().zip(&weights)) {
            if *w > 0.0 {
                normalize(sum);
                *center = std::mem::take(sum);
            }
        }
    }
    centers
}

/// Assign a point to the index of its most-similar center.
fn assign(v: &[f32], centers: &[Vec<f32>]) -> usize {
    let mut best = 0;
    let mut best_sim = f32::NEG_INFINITY;
    for (i, c) in centers.iter().enumerate() {
        let s = cosine(v, c);
        if s > best_sim {
            best_sim = s;
            best = i;
        }
    }
    best
}

/// k-means++ seeding (weighted): spreads initial centers by confidence × distance²,
/// which converges better than uniform picks on tight taste clusters.
fn kmeanspp_init(points: &[(Vec<f32>, f32)], k: usize, rng: &mut StdRng) -> Vec<Vec<f32>> {
    let mut centers: Vec<Vec<f32>> = Vec::with_capacity(k);
    let first = weighted_pick(points.iter().map(|(_, w)| *w), rng).unwrap_or(0);
    centers.push(points[first].0.clone());

    while centers.len() < k {
        let scores = points.iter().map(|(v, w)| {
            let nearest = centers
                .iter()
                .map(|c| cosine(v, c))
                .fold(f32::NEG_INFINITY, f32::max);
            // squared euclidean distance between unit vectors = 2(1 - cosine)
            let d2 = (2.0 * (1.0 - nearest)).max(0.0);
            d2 * w
        });
        let idx = weighted_pick(scores, rng).unwrap_or(0);
        centers.push(points[idx].0.clone());
    }
    centers
}

/// Pick an index with probability proportional to its weight. Falls back to a
/// uniform pick when every weight is zero, and `None` only for an empty input.
fn weighted_pick(weights: impl Iterator<Item = f32>, rng: &mut StdRng) -> Option<usize> {
    let w: Vec<f32> = weights.map(|x| x.max(0.0)).collect();
    if w.is_empty() {
        return None;
    }
    let total: f32 = w.iter().sum();
    if total <= 0.0 {
        return Some(rng.random_range(0..w.len()));
    }
    let mut r = rng.random::<f32>() * total;
    for (i, x) in w.iter().enumerate() {
        r -= x;
        if r <= 0.0 {
            return Some(i);
        }
    }
    Some(w.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::nearest_centroid;

    #[test]
    fn few_points_each_become_a_centroid() {
        let pts = vec![(vec![1.0, 0.0], 1.0), (vec![0.0, 1.0], 1.0)];
        let c = taste_centroids(&pts, 3, 0);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn separates_two_clusters() {
        // Two tight clusters around orthogonal axes + confidence weights.
        let mut pts = Vec::new();
        for i in 0u8..10 {
            let j = f32::from(i) * 0.01;
            pts.push((crate::normalized(&[1.0, j]), 1.0));
            pts.push((crate::normalized(&[j, 1.0]), 1.0));
        }
        let centroids = taste_centroids(&pts, 2, 42);
        assert_eq!(centroids.len(), 2);
        // Each axis-aligned probe should land very close to one centroid.
        assert!(nearest_centroid(&[1.0, 0.0], &centroids) > 0.95);
        assert!(nearest_centroid(&[0.0, 1.0], &centroids) > 0.95);
    }

    #[test]
    fn deterministic_for_seed() {
        let pts: Vec<_> = (0u8..20)
            .map(|i| (crate::normalized(&[1.0, f32::from(i)]), 1.0))
            .collect();
        assert_eq!(taste_centroids(&pts, 3, 7), taste_centroids(&pts, 3, 7));
    }
}
