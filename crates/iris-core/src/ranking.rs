// Effective-size maths casts a positive, bounded byte count to f64 for
// a ratio comparison — precision loss on values that large is irrelevant
// to the ordering.
#![allow(clippy::cast_precision_loss)]

//! Shared "recommended" ordering policy for release candidates.
//!
//! One policy, used in three places so they can never drift apart:
//!   * the search default ("Recommended") ordering — applied as the
//!     tie-break *after* relevance (`iris-api/src/ranking.rs`),
//!   * the notify scheduler's best-per-`(season, episode, language)`
//!     pick (`iris-api/src/collections_scheduler.rs`),
//!   * the season-pack / singleton grab selection
//!     (`iris-db/src/available_episodes.rs`).
//!
//! Policy (decided with the user): **smallest size first**, with seeders
//! acting only as a *garde-fou* — a release must look *alive*
//! (`>= `[`SEED_FLOOR`] seeders, or unknown) and *sane*
//! (`>= `[`MIN_SANE_BYTES`]) before it can win on size; among otherwise
//! equal candidates, more seeders breaks the tie. `MULTi` releases get
//! their effective size discounted by [`MULTI_SIZE_DISCOUNT`] so they
//! edge out a same-size single-language release, while a markedly
//! lighter single-language release still ranks ahead.

use std::cmp::Ordering;

/// Minimum seeders for a release to count as "alive". Below this (and
/// not unknown) it is demoted beneath every alive candidate, so the
/// size-first rule can never resurrect a near-dead torrent. A 6 GB pack
/// with 50 seeders must still beat an 8 GB pack with 200 — but a 6 GB
/// pack with 1 seeder must not beat a healthy 8 GB one.
pub const SEED_FLOOR: i64 = 3;

/// Anti-junk floor: a release smaller than this is almost certainly a
/// sample / nfo-only and must never win the "smallest" race.
pub const MIN_SANE_BYTES: i64 = 50 * 1024 * 1024;

/// `MULTi` effective-size discount — a `MULTi` release is ranked as though
/// it were this many times smaller, so it wins ties against a same-size
/// single-language release without overriding a much lighter one.
pub const MULTI_SIZE_DISCOUNT: f64 = 1.5;

/// A release reduced to just the fields the recommended ordering needs.
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    pub seeders: Option<i64>,
    pub size_bytes: Option<i64>,
    /// `true` when the release is tagged `MULTi` (multi-audio).
    pub is_multi: bool,
}

impl Candidate {
    /// Looks alive: `>= `[`SEED_FLOOR`] seeders, or unknown (we can't
    /// confirm those are dead). Public so grab-path comparators can
    /// demote low-seeded candidates *after* their own leading criteria
    /// (format / codec preference) instead of before.
    #[must_use]
    pub fn alive(self) -> bool {
        // Unknown seeder count is kept (we can't confirm it's dead) —
        // same contract as the `available_episodes` cache.
        self.seeders.is_none_or(|s| s >= SEED_FLOOR)
    }

    /// Not junk-sized (`>= `[`MIN_SANE_BYTES`], or unknown). Public for
    /// the same reason as [`Self::alive`] — this guard stays absolute in
    /// grab comparators while aliveness is allowed to rank lower.
    #[must_use]
    pub fn big_enough(self) -> bool {
        self.size_bytes.is_none_or(|b| b >= MIN_SANE_BYTES)
    }

    /// Alive *and* not junk-sized: only sane candidates compete on the
    /// smallest-size rule. Dodgy ones sort beneath every sane one.
    /// Public so grab-path comparators that add extra criteria (format /
    /// codec preference) can keep sanity as their leading key.
    #[must_use]
    pub fn sane(self) -> bool {
        self.alive() && self.big_enough()
    }

    /// Effective size in GiB, MULTi-discounted. Unknown size sorts last
    /// (treated as infinitely large) so a sized release always wins.
    fn eff_size_gib(self) -> f64 {
        let gib = self
            .size_bytes
            .map_or(f64::INFINITY, |b| b.max(0) as f64 / 1_073_741_824.0);
        if self.is_multi {
            gib / MULTI_SIZE_DISCOUNT
        } else {
            gib
        }
    }

    fn seeder_count(self) -> i64 {
        self.seeders.unwrap_or(0)
    }
}

/// Compare two candidates by the recommended policy. The **better**
/// candidate (the one that should be offered / sort first) compares as
/// [`Ordering::Less`], so this can be handed straight to `sort_by` /
/// `min_by` / `max_by` (best = minimum).
#[must_use]
pub fn recommended_cmp(a: &Candidate, b: &Candidate) -> Ordering {
    // 1. Sane (alive + big enough) before anything dodgy.
    b.sane()
        .cmp(&a.sane())
        // 2. Smallest effective size first.
        .then_with(|| {
            a.eff_size_gib()
                .partial_cmp(&b.eff_size_gib())
                .unwrap_or(Ordering::Equal)
        })
        // 3. More seeders breaks the tie.
        .then_with(|| b.seeder_count().cmp(&a.seeder_count()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: i64 = 1_073_741_824;

    fn c(seeders: i64, size_gib: i64, is_multi: bool) -> Candidate {
        Candidate {
            seeders: Some(seeders),
            size_bytes: Some(size_gib * GIB),
            is_multi,
        }
    }

    /// Returns the candidate that should be offered first.
    fn best(a: Candidate, b: Candidate) -> Candidate {
        if recommended_cmp(&a, &b) == Ordering::Less {
            a
        } else {
            b
        }
    }

    #[test]
    fn smaller_alive_beats_bigger_with_more_seeders() {
        // 6 GB / 50 seeders must beat 8 GB / 200 and 51 GB / 500.
        let light = c(50, 6, false);
        let mid = c(200, 8, false);
        let monster = c(500, 51, false);
        assert_eq!(best(light, mid).size_bytes, light.size_bytes);
        assert_eq!(best(light, monster).size_bytes, light.size_bytes);
        assert_eq!(best(mid, monster).size_bytes, mid.size_bytes);
    }

    #[test]
    fn near_dead_loses_despite_being_smaller() {
        // 6 GB but only 1 seeder (below floor) loses to a healthy 8 GB.
        let near_dead = c(1, 6, false);
        let healthy = c(200, 8, false);
        assert_eq!(best(near_dead, healthy).seeders, healthy.seeders);
    }

    #[test]
    fn junk_sized_loses_despite_being_smallest() {
        let junk = Candidate {
            seeders: Some(500),
            size_bytes: Some(10 * 1024 * 1024), // 10 MB < floor
            is_multi: false,
        };
        let real = c(50, 6, false);
        assert_eq!(best(junk, real).size_bytes, real.size_bytes);
    }

    #[test]
    fn multi_edges_same_size_single_language() {
        let multi = c(50, 8, true);
        let single = c(50, 8, false);
        // 8 GB MULTi ranks as ~5.3 GB → beats 8 GB single-language.
        assert!(best(multi, single).is_multi);
    }

    #[test]
    fn much_lighter_single_language_still_beats_multi() {
        let multi = c(50, 12, true); // effective ~8 GB
        let light_single = c(50, 6, false);
        assert!(!best(multi, light_single).is_multi);
    }

    #[test]
    fn more_seeders_breaks_exact_size_tie() {
        let a = c(50, 8, false);
        let b = c(200, 8, false);
        assert_eq!(best(a, b).seeders, b.seeders);
    }

    #[test]
    fn unknown_size_sorts_last() {
        let sized = c(50, 40, false);
        let no_size = Candidate {
            seeders: Some(500),
            size_bytes: None,
            is_multi: false,
        };
        assert_eq!(best(sized, no_size).size_bytes, sized.size_bytes);
    }
}
