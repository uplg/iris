// Score arithmetic mixes i64 (DB), u32/u16 (parsed), u64 (size_bytes)
// and f64 (final composite). Values are domain-bounded and we only
// need approximate ordering — pedantic numeric-cast warnings are
// noise here.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
)]

//! Search result post-processing — runs once per `/api/search` call
//! after the provider fan-out has aggregated. Two responsibilities:
//!
//!   1. **Relevance ranking.** Re-sort results by how well they match
//!      the SCENE-parsed user query (`Classroom of the Elite S04E11`
//!      → title + season + episode hints). Without this, season packs
//!      with hundreds of seeders out-score the requested single
//!      episode purely on the seeders/size composite — the
//!      "Recommended" tab used to surface noise that way before the
//!      UNIT3D providers shipped and the problem got worse.
//!   2. **Library dedup.** Set [`SearchResult::already_in_library`]
//!      when the result's SCENE identity is already an
//!      `episode_files` row. Prevents the surprisingly-common second
//!      ingest of the same episode under a different release group,
//!      which the user flagged after watching it happen in practice.
//!
//! Ranking only fires when the caller didn't explicitly pick a sort
//! (`sort_by.is_none()`); when the user clicks "Seeders" / "Newest" /
//! "Smallest" / "Title", we honour that and skip the relevance
//! re-sort — the dedup flag still gets set regardless.

use std::collections::HashMap;

use iris_core::search::{SearchQuery, SearchResult};
use iris_db::episode_files::LibraryEpisodeKey;
use iris_media::filename::{parse, series_key};
use iris_providers::registry::{AggregatedResults, ParsedQueryInfo};
use sqlx::SqlitePool;

/// One on-disk episode the dedup logic can match against.
#[derive(Debug, Clone)]
struct LibraryHit {
    infohash: String,
    file_idx: u32,
}

/// `(normalized_collection_title, season, episode) → (infohash, file_idx)`
/// index of every episode currently on disk. Built once per search
/// request from `episode_files JOIN collections`. The household scale
/// keeps this in the low thousands at most so a `HashMap` is plenty.
#[derive(Debug, Default)]
pub struct LibraryIndex {
    by_key: HashMap<(String, u32, u32), LibraryHit>,
}

impl LibraryIndex {
    pub async fn load(pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        let rows = iris_db::episode_files::list_library_keys(pool).await?;
        Ok(Self::from_rows(rows))
    }

    pub fn empty() -> Self {
        Self::default()
    }

    fn from_rows(rows: Vec<LibraryEpisodeKey>) -> Self {
        let mut by_key = HashMap::with_capacity(rows.len());
        for r in rows {
            // Season-pack rows (episode == 0) shouldn't dedup
            // single-episode searches — keep them out of the index.
            if r.season >= 0 && r.episode > 0 && r.file_idx >= 0 {
                by_key.insert(
                    (r.normalized_title, r.season as u32, r.episode as u32),
                    LibraryHit {
                        infohash: r.infohash,
                        file_idx: r.file_idx as u32,
                    },
                );
            }
        }
        Self { by_key }
    }

    fn lookup(&self, title_norm: &str, season: u32, episode: u32) -> Option<&LibraryHit> {
        self.by_key.get(&(title_norm.to_string(), season, episode))
    }
}

/// Project the parsed-query summary the frontend renders as a banner
/// ("Showing results for *Classroom of the Elite* · S04E11"). Returns
/// `None` when the parser produced nothing usable.
pub fn parsed_query_summary(q: &SearchQuery) -> Option<ParsedQueryInfo> {
    let title = q.parsed_title.as_ref()?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    // Only surface the summary when there's something the user would
    // recognise as "structured" — bare titles don't need a banner.
    if q.season.is_none() && q.episode.is_none() && q.year.is_none() {
        return None;
    }
    Some(ParsedQueryInfo {
        title,
        season: q.season,
        episode: q.episode,
        year: q.year,
    })
}

/// Re-rank `agg.results` in place by relevance to `q`, and set the
/// library-dedup flag on each matching result.
pub fn rerank_results(agg: &mut AggregatedResults, q: &SearchQuery, lib: &LibraryIndex) {
    // User-chosen sort wins; we only own the default "relevance" mode.
    let relevance_mode = q.sort_by.is_none();

    let mut scored: Vec<(f64, SearchResult)> = Vec::with_capacity(agg.results.len());
    for mut r in agg.results.drain(..) {
        let parsed = parse(&r.title);
        let result_title = parsed.as_ref().map(|p| series_key(&p.title));
        let result_season = parsed.as_ref().and_then(|p| p.season);
        let result_episode = parsed.as_ref().and_then(|p| p.episode);
        let result_year = parsed.as_ref().and_then(|p| p.year).or(r.year);

        if let (Some(t), Some(s), Some(e)) = (result_title.as_deref(), result_season, result_episode)
        {
            // episode == 0 is the in-band season-pack sentinel from
            // the SCENE parser — never let that hit dedup, otherwise
            // an S04 pack hides every S04Exx single-episode search.
            if e > 0 {
                if let Some(hit) = lib.lookup(t, s, e) {
                    r.already_in_library = true;
                    r.library_infohash = Some(hit.infohash.clone());
                    r.library_file_idx = Some(hit.file_idx);
                }
            }
        }

        let score = compute_score(
            q,
            &r,
            result_title.as_deref(),
            result_season,
            result_episode,
            result_year,
        );
        scored.push((score, r));
    }

    if relevance_mode {
        // Stable-ish sort: NaN never happens in our score path
        // (composite is always finite), but guard against a future
        // arithmetic surprise by treating equal/non-comparable as Equal.
        scored.sort_by(|(a, _), (b, _)| {
            b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    agg.results = scored.into_iter().map(|(_, r)| r).collect();
}

/// Score breakdown (additive, no normalisation — we only need a
/// stable ordering, not a bounded probability):
///
/// - title match: +200 exact, +80 substring, 0 unrelated
/// - `SxxExx` match: +150 both, +80 season-only / pack accept,
///   -50 conflicting S/E
/// - year match: +30
/// - popularity: `seeders / √max(size_GiB, 0.5)`
/// - non-video penalty: -100 when size < 200 MB and kind is unknown
/// - dedup penalty: -250 when already in library (keep visible,
///   but well below fresh candidates)
fn compute_score(
    q: &SearchQuery,
    r: &SearchResult,
    result_title: Option<&str>,
    result_season: Option<u32>,
    result_episode: Option<u32>,
    result_year: Option<u16>,
) -> f64 {
    let mut score = 0.0_f64;

    if let (Some(qt), Some(rt)) = (q.parsed_title.as_deref(), result_title) {
        if !qt.is_empty() && !rt.is_empty() {
            if qt == rt {
                score += 200.0;
            } else if rt.contains(qt) || qt.contains(rt) {
                score += 80.0;
            }
        }
    }

    match (q.season, q.episode, result_season, result_episode) {
        // Exact S/E match.
        (Some(qs), Some(qe), Some(rs), Some(re)) if qs == rs && qe == re && qe != 0 => {
            score += 150.0;
        }
        // Query wanted SxxExx; result is the matching season pack
        // (episode == 0 sentinel). Acceptable but not as good.
        (Some(qs), Some(_), Some(rs), Some(0)) if qs == rs => {
            score += 80.0;
        }
        // Query was "Show S04" with no episode: any S04 result wins.
        (Some(qs), None | Some(0), Some(rs), _) if qs == rs => {
            score += 80.0;
        }
        // Both sides have S/E but they disagree — push down.
        (Some(qs), Some(qe), Some(rs), Some(re)) if qs != rs || qe != re => {
            let _ = (qs, qe, rs, re);
            score -= 50.0;
        }
        _ => {}
    }

    if let (Some(qy), Some(ry)) = (q.year, result_year) {
        if qy == ry {
            score += 30.0;
        }
    }

    let seeders = f64::from(r.seeders.unwrap_or(0));
    let size_gib = r
        .size_bytes
        .map_or(0.0, |b| b as f64 / 1_073_741_824.0);
    let denom = size_gib.max(0.5).sqrt();
    score += seeders / denom;

    let likely_non_video =
        r.size_bytes.is_some_and(|b| b < 200 * 1024 * 1024) && r.kind.is_none();
    if likely_non_video {
        score -= 100.0;
    }

    if r.already_in_library {
        score -= 250.0;
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_core::search::MediaKind;

    fn mk_result(title: &str, seeders: u32, size_gib: f64, kind: Option<MediaKind>) -> SearchResult {
        SearchResult {
            provider_id: "test".into(),
            external_id: title.into(),
            title: title.into(),
            year: None,
            size_bytes: Some((size_gib * 1_073_741_824.0) as u64),
            seeders: Some(seeders),
            leechers: None,
            infohash: None,
            magnet: None,
            category: None,
            tags: Vec::new(),
            freeleech: false,
            uploader: None,
            uploaded_at: None,
            tmdb_id: None,
            kind,
            poster_url: None,
            already_in_library: false,
            library_infohash: None,
            library_file_idx: None,
            language: None,
        }
    }

    fn mk_query(q: &str, season: Option<u32>, episode: Option<u32>) -> SearchQuery {
        SearchQuery {
            q: q.into(),
            page: None,
            limit: None,
            sort_by: None,
            order: None,
            kind: None,
            parsed_title: Some(series_key(q.split_whitespace().take(4).collect::<Vec<_>>().join(" ").as_str())),
            season,
            episode,
            year: None,
        }
    }

    #[test]
    fn exact_episode_beats_season_pack() {
        // Episode user typed: classroom S04E11.
        let q = SearchQuery {
            q: "classroom of the elite S04E11".into(),
            page: None,
            limit: None,
            sort_by: None,
            order: None,
            kind: None,
            parsed_title: Some(series_key("classroom of the elite")),
            season: Some(4),
            episode: Some(11),
            year: None,
        };
        // S04 pack: massive seeders, huge size.
        let pack = mk_result(
            "Classroom.of.the.Elite.S04.MULTi.1080p.WEB.AAC.x264-XYZ",
            200,
            12.0,
            Some(MediaKind::Tv),
        );
        // The specific episode: half the seeders, much smaller.
        let ep = mk_result(
            "Classroom.of.the.Elite.S04E11.MULTi.1080p.WEB.AAC.x264-Tsundere-Raws",
            80,
            1.0,
            Some(MediaKind::Tv),
        );

        let mut agg = AggregatedResults {
            results: vec![pack, ep],
            ..Default::default()
        };
        rerank_results(&mut agg, &q, &LibraryIndex::empty());

        assert_eq!(
            agg.results[0].title,
            "Classroom.of.the.Elite.S04E11.MULTi.1080p.WEB.AAC.x264-Tsundere-Raws",
            "exact SxxExx must outrank the season pack even with fewer seeders",
        );
    }

    #[test]
    fn dedup_flags_and_demotes_owned_episode() {
        // User searches the whole season; library has S04E11. The
        // S04E11 candidate must be flagged (with the library infohash
        // attached for "Play existing" UI), and a fresh S04E12 wins
        // the #1 spot because it isn't demoted.
        let q = SearchQuery {
            q: "classroom of the elite S04".into(),
            page: None,
            limit: None,
            sort_by: None,
            order: None,
            kind: None,
            parsed_title: Some(series_key("classroom of the elite")),
            season: Some(4),
            episode: None,
            year: None,
        };
        let owned_e11 = mk_result(
            "Classroom.of.the.Elite.S04E11.MULTi.1080p.WEB.AAC.x264-Tsundere-Raws",
            120,
            1.0,
            Some(MediaKind::Tv),
        );
        let fresh_e12 = mk_result(
            "Classroom.of.the.Elite.S04E12.VOSTFR.1080p.WEBRip.x265-TLC",
            40,
            1.2,
            Some(MediaKind::Tv),
        );

        let lib = LibraryIndex::from_rows(vec![LibraryEpisodeKey {
            normalized_title: series_key("classroom of the elite"),
            season: 4,
            episode: 11,
            infohash: "abc".into(),
            file_idx: 0,
        }]);
        let mut agg = AggregatedResults {
            results: vec![owned_e11, fresh_e12],
            ..Default::default()
        };
        rerank_results(&mut agg, &q, &lib);

        assert!(
            agg.results[0].title.contains("S04E12"),
            "fresh episode should outrank the demoted owned one",
        );
        let owned = agg
            .results
            .iter()
            .find(|r| r.title.contains("S04E11"))
            .expect("owned episode still present");
        assert!(owned.already_in_library, "owned episode must be flagged");
        assert_eq!(owned.library_infohash.as_deref(), Some("abc"));
    }

    #[test]
    fn dedup_flags_every_release_of_same_episode() {
        // Searching the exact episode the user already has: every
        // candidate parses to the same (title, S, E), so every
        // candidate gets flagged. The ranking falls back to quality
        // (seeders/√size) — UI is responsible for showing "Play
        // existing" on the row with `library_infohash`.
        let q = SearchQuery {
            q: "classroom S04E11".into(),
            page: None,
            limit: None,
            sort_by: None,
            order: None,
            kind: None,
            parsed_title: Some(series_key("classroom of the elite")),
            season: Some(4),
            episode: Some(11),
            year: None,
        };
        let r1 = mk_result(
            "Classroom.of.the.Elite.S04E11.MULTi.1080p.WEB.AAC.x264-Tsundere-Raws",
            120,
            1.0,
            Some(MediaKind::Tv),
        );
        let r2 = mk_result(
            "Classroom.of.the.Elite.S04E11.VOSTFR.1080p.WEBRip.x265-TLC",
            40,
            1.2,
            Some(MediaKind::Tv),
        );
        let lib = LibraryIndex::from_rows(vec![LibraryEpisodeKey {
            normalized_title: series_key("classroom of the elite"),
            season: 4,
            episode: 11,
            infohash: "abc".into(),
            file_idx: 0,
        }]);
        let mut agg = AggregatedResults {
            results: vec![r1, r2],
            ..Default::default()
        };
        rerank_results(&mut agg, &q, &lib);
        assert!(agg.results.iter().all(|r| r.already_in_library));
        assert!(
            agg.results
                .iter()
                .all(|r| r.library_infohash.as_deref() == Some("abc")),
            "every candidate points at the same on-disk infohash",
        );
    }

    #[test]
    fn season_pack_is_not_indexed_for_dedup() {
        // A library-side season pack (episode == 0 sentinel) must NOT
        // mask a single-episode search — only real episodes hit the
        // dedup map.
        let lib = LibraryIndex::from_rows(vec![LibraryEpisodeKey {
            normalized_title: series_key("squid game"),
            season: 1,
            episode: 0,
            infohash: "pack".into(),
            file_idx: 0,
        }]);
        assert!(lib.lookup("squid game", 1, 5).is_none());
    }

    #[test]
    fn explicit_sort_skips_relevance_resort() {
        let mut q = mk_query("classroom S04E11", Some(4), Some(11));
        q.sort_by = Some(iris_core::search::SortField::Seeders);
        let pack = mk_result(
            "Classroom.of.the.Elite.S04.MULTi.1080p.WEB.AAC.x264-XYZ",
            200,
            12.0,
            Some(MediaKind::Tv),
        );
        let ep = mk_result(
            "Classroom.of.the.Elite.S04E11.MULTi.1080p.WEB.AAC.x264-Tsundere-Raws",
            80,
            1.0,
            Some(MediaKind::Tv),
        );
        let mut agg = AggregatedResults {
            results: vec![pack.clone(), ep.clone()],
            ..Default::default()
        };
        rerank_results(&mut agg, &q, &LibraryIndex::empty());
        // Order preserved (no relevance re-sort).
        assert_eq!(agg.results[0].title, pack.title);
        assert_eq!(agg.results[1].title, ep.title);
    }

    #[test]
    fn parsed_query_summary_is_silent_for_bare_titles() {
        let q = SearchQuery {
            q: "classroom of the elite".into(),
            page: None,
            limit: None,
            sort_by: None,
            order: None,
            kind: None,
            parsed_title: Some(series_key("classroom of the elite")),
            season: None,
            episode: None,
            year: None,
        };
        assert!(parsed_query_summary(&q).is_none());
    }

    #[test]
    fn parsed_query_summary_surfaces_se() {
        let q = SearchQuery {
            q: "classroom S04E11".into(),
            page: None,
            limit: None,
            sort_by: None,
            order: None,
            kind: None,
            parsed_title: Some("classroom".into()),
            season: Some(4),
            episode: Some(11),
            year: None,
        };
        let s = parsed_query_summary(&q).unwrap();
        assert_eq!(s.title, "classroom");
        assert_eq!(s.season, Some(4));
        assert_eq!(s.episode, Some(11));
    }
}
