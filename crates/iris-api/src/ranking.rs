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

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use iris_core::search::{SearchQuery, SearchResult};
use iris_db::episode_files::LibraryEpisodeKey;
use iris_media::filename::{Language, detect_language, parse, series_key};
use iris_providers::registry::{AggregatedResults, ParsedQueryInfo};
use sqlx::SqlitePool;

/// Identity-level "what's already on disk" index for search dedup.
///
/// Dedup is keyed strictly on **infohash** — a result is "already in
/// library" only when it is the *exact same torrent* we already hold.
/// A different release group, resolution, or language of the same
/// episode is a different infohash and stays fully grabbable. (We
/// deliberately do NOT dedup on `(title, season, episode)`: that masked
/// an FR release because the EN one was owned, blocking the download —
/// the exact bug this index was reworked to fix.)
///
/// `file_idx_by_infohash` is a best-effort `infohash → playable file`
/// map drawn from `episode_files`, so the UI's "Play existing" button
/// can deep-link straight into the file. Absent for movies / packs with
/// no episode-file row — the frontend then falls back to the preview
/// dialog. Built once per search request; household scale keeps it tiny.
#[derive(Debug, Default)]
pub struct LibraryIndex {
    owned_infohashes: HashSet<String>,
    file_idx_by_infohash: HashMap<String, u32>,
}

impl LibraryIndex {
    pub async fn load(pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        let infohashes = iris_db::torrents::list_active_infohashes(pool).await?;
        let keys = iris_db::episode_files::list_library_keys(pool).await?;
        Ok(Self::build(infohashes, keys))
    }

    pub fn empty() -> Self {
        Self::default()
    }

    fn build(infohashes: Vec<String>, keys: Vec<LibraryEpisodeKey>) -> Self {
        let owned_infohashes: HashSet<String> =
            infohashes.into_iter().map(|h| h.to_ascii_lowercase()).collect();
        let mut file_idx_by_infohash = HashMap::new();
        for k in keys {
            if k.file_idx < 0 {
                continue;
            }
            // First episode-file wins; any playable index into the
            // owned torrent is fine for the "Play existing" deep link.
            file_idx_by_infohash
                .entry(k.infohash.to_ascii_lowercase())
                .or_insert(k.file_idx as u32);
        }
        Self {
            owned_infohashes,
            file_idx_by_infohash,
        }
    }

    /// `Some(match)` when `infohash` is an owned torrent (its
    /// `file_idx` carries a playable file index when one is known);
    /// `None` when we don't hold this exact torrent.
    fn lookup(&self, infohash: &str) -> Option<LibraryMatch> {
        let h = infohash.to_ascii_lowercase();
        self.owned_infohashes.contains(&h).then(|| LibraryMatch {
            file_idx: self.file_idx_by_infohash.get(&h).copied(),
        })
    }
}

/// A search result matched an owned torrent by infohash. `file_idx` is
/// the playable file for the "Play existing" deep link, when known.
#[derive(Debug, Clone, Copy)]
struct LibraryMatch {
    file_idx: Option<u32>,
}

/// Extract a lowercase hex (40-char v1) btih from a magnet URI's
/// `xt=urn:btih:` parameter. Returns `None` for base32 / v2 hashes —
/// we only dedup when we can compare like-for-like with the hex
/// infohash librqbit stores.
fn infohash_from_magnet(magnet: &str) -> Option<String> {
    let lower = magnet.to_ascii_lowercase();
    let start = lower.find("xt=urn:btih:")? + "xt=urn:btih:".len();
    let hash: String = lower[start..]
        .chars()
        .take_while(|c| *c != '&')
        .collect();
    (hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit())).then_some(hash)
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

    let mut scored: Vec<(f64, iris_core::ranking::Candidate, SearchResult)> =
        Vec::with_capacity(agg.results.len());
    for mut r in agg.results.drain(..) {
        let parsed = parse(&r.title);
        let result_title = parsed.as_ref().map(|p| series_key(&p.title));
        let result_season = parsed.as_ref().and_then(|p| p.season);
        let result_episode = parsed.as_ref().and_then(|p| p.episode);
        let result_year = parsed.as_ref().and_then(|p| p.year).or(r.year);
        // Compute the recommended-ordering view once here (it scans the
        // title for the MULTi tag) rather than on every sort comparison.
        let cand = candidate(&r);

        // Dedup is infohash-only: flag the result solely when it is the
        // exact torrent already on disk. A different release/language of
        // the same episode is a distinct infohash and stays grabbable.
        let result_infohash = r
            .infohash
            .clone()
            .or_else(|| r.magnet.as_deref().and_then(infohash_from_magnet));
        if let Some(ih) = result_infohash {
            if let Some(m) = lib.lookup(&ih) {
                r.already_in_library = true;
                r.library_infohash = Some(ih);
                r.library_file_idx = m.file_idx;
            }
        }

        // Expose the parsed (season, episode) on the result so the
        // web search grid can render a compact "S04E11" chip per
        // card without re-running the parser client-side. We've
        // already parsed once for ranking — share the work.
        r.parsed_season = result_season;
        r.parsed_episode = result_episode;

        let score = relevance_score(
            q,
            &r,
            result_title.as_deref(),
            result_season,
            result_episode,
            result_year,
        );
        scored.push((score, cand, r));
    }

    if relevance_mode {
        // Order: relevance first, then the shared "recommended" policy
        // (smallest size first, seeders as garde-fou, MULTi discounted)
        // as the tie-break, then title for a deterministic result. NaN
        // never happens in our score path (relevance is always finite),
        // but treat non-comparable as Equal defensively.
        scored.sort_by(|(sa, ca, ra), (sb, cb, rb)| {
            sb.partial_cmp(sa)
                .unwrap_or(Ordering::Equal)
                .then_with(|| iris_core::ranking::recommended_cmp(ca, cb))
                .then_with(|| ra.title.cmp(&rb.title))
        });
    }

    agg.results = scored.into_iter().map(|(_, _, r)| r).collect();
}

/// Build the [`iris_core::ranking::Candidate`] view of a result for the
/// recommended tie-break: seeders + size + whether it's a `MULTi` release
/// (so `MULTi` gets the effective-size discount). Language is read from the
/// SCENE title via the shared `detect_language`.
fn candidate(r: &SearchResult) -> iris_core::ranking::Candidate {
    iris_core::ranking::Candidate {
        seeders: r.seeders.map(i64::from),
        size_bytes: r.size_bytes.map(|b| i64::try_from(b).unwrap_or(i64::MAX)),
        is_multi: detect_language(&r.title) == Language::Multi,
    }
}

/// Pure **relevance** score (additive, no normalisation — we only need a
/// stable ordering). Popularity is deliberately NOT folded in here: size
/// and seeders are handled by [`iris_core::ranking::recommended_cmp`] as
/// the tie-break *after* relevance, so a relevant-but-huge season pack no
/// longer out-scores a relevant, lighter release on raw seeders.
///
/// - title match: +200 exact, +80 substring, 0 unrelated
/// - `SxxExx` match: +150 both, +80 season-only / pack accept,
///   -50 conflicting S/E
/// - year match: +30
/// - non-video penalty: -100 when size < 200 MB and kind is unknown
/// - dedup penalty: -250 when already in library (keep visible,
///   but well below fresh candidates)
fn relevance_score(
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
            download_url: None,
            parsed_season: None,
            parsed_episode: None,
        }
    }

    fn with_infohash(mut r: SearchResult, ih: &str) -> SearchResult {
        r.infohash = Some(ih.into());
        r
    }

    /// Build a `LibraryIndex` from owned infohashes plus `(infohash,
    /// file_idx)` episode-file rows.
    fn lib_with(infohashes: &[&str], files: &[(&str, i64)]) -> LibraryIndex {
        LibraryIndex::build(
            infohashes.iter().map(|s| (*s).to_string()).collect(),
            files
                .iter()
                .map(|(ih, idx)| LibraryEpisodeKey {
                    normalized_title: String::new(),
                    season: 0,
                    episode: 0,
                    infohash: (*ih).to_string(),
                    file_idx: *idx,
                })
                .collect(),
        )
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
    fn lighter_release_wins_when_relevance_ties() {
        // Two exact-title movie matches → identical relevance. The
        // lighter, still-alive release wins even with far fewer seeders:
        // the size-first "recommended" tie-break, so a 50 GB 4K rip no
        // longer beats an 8 GB 1080p one on raw seeders.
        let q = mk_query("the matrix", None, None);
        let monster = mk_result("The.Matrix.2160p.UHD.BluRay.x265-GRP", 800, 50.0, Some(MediaKind::Movie));
        let light = mk_result("The.Matrix.1080p.BluRay.x264-GRP", 40, 8.0, Some(MediaKind::Movie));
        let mut agg = AggregatedResults {
            results: vec![monster, light],
            ..Default::default()
        };
        rerank_results(&mut agg, &q, &LibraryIndex::empty());
        assert!(
            agg.results[0].title.contains("1080p"),
            "lighter alive release must win the relevance tie",
        );
    }

    #[test]
    fn multi_wins_tie_against_same_size_single_language() {
        // Same title, same size, comparable seeders: MULTi edges ahead
        // via the effective-size discount.
        let q = mk_query("dune part two", None, None);
        let single = mk_result("Dune.Part.Two.2024.VOSTFR.1080p.BluRay.x264-GRP", 100, 10.0, Some(MediaKind::Movie));
        let multi = mk_result("Dune.Part.Two.2024.MULTi.1080p.BluRay.x264-GRP", 90, 10.0, Some(MediaKind::Movie));
        let mut agg = AggregatedResults {
            results: vec![single, multi],
            ..Default::default()
        };
        rerank_results(&mut agg, &q, &LibraryIndex::empty());
        assert!(
            agg.results[0].title.contains("MULTi"),
            "MULTi must win a same-size, same-relevance tie",
        );
    }

    #[test]
    fn dedup_flags_and_demotes_owned_torrent() {
        // User searches the whole season; library holds the exact
        // S04E11 torrent (infohash "abc"). That candidate is flagged
        // (with the library infohash + file_idx for "Play existing"),
        // and a fresh S04E12 — a torrent we don't own — wins #1.
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
        let owned_e11 = with_infohash(
            mk_result(
                "Classroom.of.the.Elite.S04E11.MULTi.1080p.WEB.AAC.x264-Tsundere-Raws",
                120,
                1.0,
                Some(MediaKind::Tv),
            ),
            "abc",
        );
        let fresh_e12 = with_infohash(
            mk_result(
                "Classroom.of.the.Elite.S04E12.VOSTFR.1080p.WEBRip.x265-TLC",
                40,
                1.2,
                Some(MediaKind::Tv),
            ),
            "xyz",
        );

        let lib = lib_with(&["abc"], &[("abc", 3)]);
        let mut agg = AggregatedResults {
            results: vec![owned_e11, fresh_e12],
            ..Default::default()
        };
        rerank_results(&mut agg, &q, &lib);

        assert!(
            agg.results[0].title.contains("S04E12"),
            "un-owned episode should outrank the demoted owned one",
        );
        let owned = agg
            .results
            .iter()
            .find(|r| r.title.contains("S04E11"))
            .expect("owned episode still present");
        assert!(owned.already_in_library, "owned torrent must be flagged");
        assert_eq!(owned.library_infohash.as_deref(), Some("abc"));
        assert_eq!(owned.library_file_idx, Some(3));
        let fresh = agg
            .results
            .iter()
            .find(|r| r.title.contains("S04E12"))
            .unwrap();
        assert!(
            !fresh.already_in_library,
            "a torrent we don't hold must stay grabbable",
        );
    }

    #[test]
    fn dedup_flags_only_the_exact_owned_torrent() {
        // Regression: owning S04E11 in MULTi must NOT flag a *different*
        // release (here a VOSTFR rip with its own infohash) of the same
        // episode. Flagging it would block grabbing the FR version —
        // the exact bug the infohash-only dedup fixes.
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
        let owned = with_infohash(
            mk_result(
                "Classroom.of.the.Elite.S04E11.MULTi.1080p.WEB.AAC.x264-Tsundere-Raws",
                120,
                1.0,
                Some(MediaKind::Tv),
            ),
            "owned-multi",
        );
        let other_release = with_infohash(
            mk_result(
                "Classroom.of.the.Elite.S04E11.VOSTFR.1080p.WEBRip.x265-TLC",
                40,
                1.2,
                Some(MediaKind::Tv),
            ),
            "other-fr",
        );
        let lib = lib_with(&["owned-multi"], &[("owned-multi", 0)]);
        let mut agg = AggregatedResults {
            results: vec![owned, other_release],
            ..Default::default()
        };
        rerank_results(&mut agg, &q, &lib);

        let owned = agg
            .results
            .iter()
            .find(|r| r.infohash.as_deref() == Some("owned-multi"))
            .unwrap();
        assert!(owned.already_in_library, "the exact owned torrent is flagged");
        let other = agg
            .results
            .iter()
            .find(|r| r.infohash.as_deref() == Some("other-fr"))
            .unwrap();
        assert!(
            !other.already_in_library,
            "a different release of the same episode stays grabbable",
        );
    }

    #[test]
    fn dedup_matches_infohash_derived_from_magnet() {
        // Results that ship only a magnet (no explicit infohash field)
        // still dedup against the owned set via the magnet's btih.
        let hash = "0123456789abcdef0123456789abcdef01234567";
        let mut r = mk_result(
            "Show.Name.S01E01.MULTi.1080p.WEB.x264-GRP",
            10,
            1.0,
            Some(MediaKind::Tv),
        );
        r.magnet = Some(format!("magnet:?xt=urn:btih:{}&dn=Show", hash.to_uppercase()));
        let lib = lib_with(&[hash], &[]);
        let mut agg = AggregatedResults {
            results: vec![r],
            ..Default::default()
        };
        rerank_results(&mut agg, &mk_query("show S01E01", Some(1), Some(1)), &lib);
        assert!(agg.results[0].already_in_library);
        assert_eq!(agg.results[0].library_file_idx, None, "no episode-file row → no deep link");
    }

    #[test]
    fn lookup_matches_only_owned_infohash() {
        let lib = lib_with(&["have"], &[("have", 2)]);
        let m = lib.lookup("HAVE").expect("owned, case-insensitive");
        assert_eq!(m.file_idx, Some(2));
        assert!(lib.lookup("missing").is_none());
    }

    #[test]
    fn infohash_from_magnet_extracts_hex_btih() {
        let hex = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            infohash_from_magnet(&format!("magnet:?xt=urn:btih:{}&dn=x", hex.to_uppercase())),
            Some(hex.to_string()),
        );
        // base32 btih (32 chars) isn't comparable to stored hex → None.
        assert_eq!(
            infohash_from_magnet("magnet:?xt=urn:btih:MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43U"),
            None,
        );
        assert_eq!(infohash_from_magnet("not a magnet"), None);
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
