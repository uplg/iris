//! Scoring a candidate embedding against a user's taste profile.

use crate::cosine;

/// Score a candidate as the cosine to its **nearest** taste centroid.
///
/// Nearest — not the mean — is the whole point of the multi-centroid profile:
/// a kids' film should score high against the household's "kids" centroid without
/// being dragged down by the "adults" centroids. An empty profile yields `0.0`
/// (cold-start user — the caller falls back to popularity/freshness shelves).
#[must_use]
pub fn nearest_centroid(item: &[f32], centroids: &[Vec<f32>]) -> f32 {
    if centroids.is_empty() {
        return 0.0;
    }
    centroids
        .iter()
        .map(|c| cosine(item, c))
        .fold(f32::NEG_INFINITY, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_profile_scores_zero() {
        assert!(nearest_centroid(&[1.0, 0.0], &[]).abs() < f32::EPSILON);
    }

    #[test]
    fn picks_the_closest_centroid() {
        let kids = vec![1.0, 0.0];
        let adults = vec![0.0, 1.0];
        let centroids = vec![kids, adults];
        // A near-kids item scores ~1 on the kids centroid, not the ~0 mean.
        assert!(nearest_centroid(&[0.99, 0.14], &centroids) > 0.9);
    }
}
