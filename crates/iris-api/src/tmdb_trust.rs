//! TMDB trust gate: TMDB artwork and titles are shown only for a match a
//! trusted signal backs — no poster rather than a wrong one.
//!
//! Signals, any one suffices:
//!
//! - **T1, tracker** — the id the tracker shipped for the release
//!   (`torrents.tmdb_id`, `tmdb_id_source = 'tracker'`). Cross-checked with
//!   the strict SCENE match: agreement is the strongest automatic signal
//!   ([`Trust::TrackerScene`]); a disagreement means no cover at all. With no
//!   SCENE match, the tracker id alone is trusted ([`Trust::Tracker`]) only
//!   when TMDB files it under the release's kind and, when the release names
//!   a year, within ±1 of it.
//! - **T2, strict SCENE** ([`Trust::Scene`]) — [`title_key`] of the parsed
//!   release title equals the key of TMDB's title or original title, the kind
//!   matches, and the year is equal when the release names one (±1 for
//!   movies: release years drift from TMDB's primary release year). Exactly
//!   one TMDB entry may pass, otherwise the match is ambiguous. No fuzzy
//!   fallback (closest year, most popular, year-less retry, `/search/multi`)
//!   ever reaches this decision.
//! - **T3, admin** ([`Trust::Admin`]) — reserved: no route sets a
//!   collection's id yet, but the column accepts it and nothing automatic
//!   overrides it.
//!
//! Conflicts ([`should_replace`]): a collection keeps its trusted id unless
//! a strictly stronger signal names another one (admin > tracker+scene >
//! scene > tracker). A legacy row (`tmdb_trust` NULL with an id) is never
//! touched automatically — only `tmdb-trust --apply` re-evaluates it.

use iris_db::SqlitePool;
use iris_db::tmdb_cache::{self, ResolveEntry};

use crate::tmdb::{MediaMetadata, TmdbClient, TmdbKind, TmdbSuggestion};

/// Cache-key prefix of strict SCENE resolutions in `tmdb_resolve_cache`.
/// Normalised names never contain `:`, so the namespace can't collide with
/// the fuzzy suggestion entries.
pub const STRICT_CACHE_PREFIX: &str = "strict:";

/// TMDB title languages the strict match compares against (`None` = TMDB's
/// default, English).
const SEARCH_LANGUAGES: [Option<&str>; 2] = [None, Some("fr-FR")];

/// An exact homonym wins a tie only with this many times the runner-up's
/// TMDB votes, and at least [`DOMINANT_MIN_VOTES`]: a release on a tracker is
/// the mainstream title, not a same-name short with three votes.
const DOMINANCE: u32 = 10;
const DOMINANT_MIN_VOTES: u32 = 50;

/// Same TTL as the suggestion cache: re-ask TMDB monthly, so a title added
/// after a negative answer gets picked up.
const MAX_AGE_DAYS: i64 = 30;

/// Which signal backs a collection's `tmdb_id`, stored in
/// `collections.tmdb_trust`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    Admin,
    TrackerScene,
    Scene,
    Tracker,
}

impl Trust {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::TrackerScene => "tracker_scene",
            Self::Scene => "scene",
            Self::Tracker => "tracker",
        }
    }

    pub fn from_stored(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Self::Admin),
            "tracker_scene" => Some(Self::TrackerScene),
            "scene" => Some(Self::Scene),
            "tracker" => Some(Self::Tracker),
            _ => None,
        }
    }

    const fn rank(self) -> u8 {
        match self {
            Self::Admin => 4,
            Self::TrackerScene => 3,
            Self::Scene => 2,
            Self::Tracker => 1,
        }
    }
}

/// The one title normalisation both sides of the T2 comparison go through:
/// compatibility decomposition with the accents dropped (`Amélie` →
/// `amelie`, full-width → ASCII), `œ`/`æ`/`ß` spelled out, lowercase,
/// apostrophes removed without a gap (`Ocean's` → `oceans`, as SCENE names
/// write it), `&` read as `and`, every other non-alphanumeric run collapsed
/// to one space. Numbers and articles are kept as is: `Part One` ≠ `Part 1`.
pub fn title_key(s: &str) -> String {
    key_with(s, false)
}

/// The keys a title may match under: [`title_key`], and the one reading an
/// apostrophe as a space, for French elisions SCENE names spell apart
/// (`C Est Magnifique` = `C'est magnifique`, `L Argent` = `L'argent`).
fn title_keys(s: &str) -> [String; 2] {
    [key_with(s, false), key_with(s, true)]
}

fn key_with(s: &str, apostrophe_splits: bool) -> String {
    use unicode_normalization::UnicodeNormalization;
    use unicode_normalization::char::is_combining_mark;

    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    let push_word = |out: &mut String, w: &str, pending: &mut bool| {
        if *pending && !out.is_empty() {
            out.push(' ');
        }
        *pending = false;
        out.push_str(w);
    };
    for c in s.nfkd().filter(|c| !is_combining_mark(*c)) {
        match c {
            '\'' | '\u{2019}' | '\u{2018}' | '\u{02BC}' | '`' | '\u{00B4}' => {
                pending_space |= apostrophe_splits;
            }
            '&' => {
                pending_space = true;
                push_word(&mut out, "and", &mut pending_space);
                pending_space = true;
            }
            'œ' | 'Œ' => push_word(&mut out, "oe", &mut pending_space),
            'æ' | 'Æ' => push_word(&mut out, "ae", &mut pending_space),
            'ß' => push_word(&mut out, "ss", &mut pending_space),
            c if c.is_alphanumeric() => {
                let mut buf = [0u8; 4];
                for low in c.to_lowercase() {
                    push_word(&mut out, low.encode_utf8(&mut buf), &mut pending_space);
                }
            }
            _ => pending_space = true,
        }
    }
    out
}

/// The single TMDB entry a strict SCENE match accepts among `candidates`,
/// or `None` (no match, or several equally good ones). See the module docs.
pub fn strict_scene_match<'a>(
    candidates: &'a [TmdbSuggestion],
    release_title: &str,
    kind: TmdbKind,
    year: Option<u32>,
) -> Option<&'a TmdbSuggestion> {
    let keys = title_keys(release_title);
    if keys[0].is_empty() {
        return None;
    }
    let same = |t: &str| title_keys(t).iter().any(|k| keys.contains(k));
    let titled: Vec<&TmdbSuggestion> = candidates
        .iter()
        .filter(|c| c.kind == kind)
        .filter(|c| same(&c.title) || c.original_title.as_deref().is_some_and(same))
        .collect();
    let unique = |tier: Vec<&'a TmdbSuggestion>| -> Option<&'a TmdbSuggestion> {
        let mut ids: Vec<&TmdbSuggestion> = Vec::with_capacity(tier.len());
        for c in tier {
            if !ids.iter().any(|i| i.tmdb_id == c.tmdb_id) {
                ids.push(c);
            }
        }
        ids.sort_by_key(|c| std::cmp::Reverse(c.vote_count.unwrap_or(0)));
        match ids.as_slice() {
            [] => None,
            [only] => Some(*only),
            [top, next, ..] => {
                let votes = top.vote_count.unwrap_or(0);
                (votes >= DOMINANT_MIN_VOTES
                    && votes
                        >= next
                            .vote_count
                            .unwrap_or(0)
                            .max(1)
                            .saturating_mul(DOMINANCE))
                .then_some(*top)
            }
        }
    };
    let Some(y) = year else {
        return unique(titled);
    };
    let exact: Vec<_> = titled
        .iter()
        .copied()
        .filter(|c| c.year == Some(y))
        .collect();
    if !exact.is_empty() || kind == TmdbKind::Tv {
        return unique(exact);
    }
    unique(
        titled
            .into_iter()
            .filter(|c| c.year.is_some_and(|cy| cy.abs_diff(y) <= 1))
            .collect(),
    )
}

/// T1 alone: TMDB files the tracker's id under the release's kind, and
/// within ±1 of the release year when it names one.
pub fn tracker_id_plausible(meta: &MediaMetadata, kind: TmdbKind, year: Option<u32>) -> bool {
    meta.kind == kind && year.is_none_or(|y| meta.year.is_some_and(|my| my.abs_diff(y) <= 1))
}

/// One tracker-shipped id and what TMDB says about it.
#[derive(Debug, Clone)]
pub struct TrackerCheck {
    pub id: u64,
    /// `None` when TMDB has no entry for it under either kind.
    pub meta: Option<MediaMetadata>,
    pub plausible: bool,
}

/// Why a decision came out the way it did — one line per report row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    TrackerAgreesWithScene,
    SceneOnly,
    TrackerOnly,
    TrackerDisagrees { tracker: Vec<u64>, scene: u64 },
    TrackersConflict(Vec<u64>),
    TrackerImplausible(Vec<u64>),
    NoSignal,
    NoTitle,
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids = |v: &[u64]| v.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
        match self {
            Self::TrackerAgreesWithScene => f.write_str("tracker id and strict SCENE match agree"),
            Self::SceneOnly => f.write_str("strict SCENE match"),
            Self::TrackerOnly => f.write_str("tracker id (kind and year check out)"),
            Self::TrackerDisagrees { tracker, scene } => write!(
                f,
                "tracker id {} disagrees with strict SCENE match {scene}",
                ids(tracker)
            ),
            Self::TrackersConflict(v) => write!(f, "tracker ids disagree: {}", ids(v)),
            Self::TrackerImplausible(v) => write!(
                f,
                "tracker id {} fails the kind/year check, no strict SCENE match",
                ids(v)
            ),
            Self::NoSignal => f.write_str("no tracker id, no strict SCENE match"),
            Self::NoTitle => f.write_str("title does not parse"),
        }
    }
}

/// T1 + T2 over one release (or every release of a collection): the
/// trusted id and its signal, or `None`, with the reason either way.
pub fn decide(scene: Option<u64>, trackers: &[TrackerCheck]) -> (Option<(u64, Trust)>, Reason) {
    let mut ids: Vec<u64> = trackers.iter().map(|t| t.id).collect();
    ids.sort_unstable();
    ids.dedup();
    if let Some(s) = scene {
        if ids.contains(&s) {
            return (
                Some((s, Trust::TrackerScene)),
                Reason::TrackerAgreesWithScene,
            );
        }
        if !ids.is_empty() {
            return (
                None,
                Reason::TrackerDisagrees {
                    tracker: ids,
                    scene: s,
                },
            );
        }
        return (Some((s, Trust::Scene)), Reason::SceneOnly);
    }
    let mut plausible: Vec<u64> = trackers
        .iter()
        .filter(|t| t.plausible)
        .map(|t| t.id)
        .collect();
    plausible.sort_unstable();
    plausible.dedup();
    match plausible.as_slice() {
        [one] => (Some((*one, Trust::Tracker)), Reason::TrackerOnly),
        [] if ids.is_empty() => (None, Reason::NoSignal),
        [] => (None, Reason::TrackerImplausible(ids)),
        _ => (None, Reason::TrackersConflict(plausible)),
    }
}

/// Should a freshly evaluated `new` match be written over what the
/// collection holds (`existing` id and trust)? Empty → yes. Legacy (an id,
/// no trust) → never: it waits for `tmdb-trust --apply`. Trusted → only for
/// a strictly stronger signal (same id: the label upgrades; another id: the
/// stronger signal wins, a tie keeps the first writer).
pub fn should_replace(existing: (Option<i64>, Option<Trust>), new: Trust) -> bool {
    match existing {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(_), Some(old)) => new.rank() > old.rank(),
    }
}

/// TMDB answered nothing usable (network down, refused): decide nothing,
/// write nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreachable;

/// What the trust gate needs from TMDB. [`TmdbClient`] in production; the
/// tests answer from fixtures.
pub(crate) trait TmdbSource {
    async fn search_typed(
        &self,
        query: &str,
        kind: TmdbKind,
        year: Option<u32>,
        language: Option<&'static str>,
    ) -> Option<Vec<TmdbSuggestion>>;

    async fn lookup(&self, id: u64, kind: TmdbKind) -> Option<Option<MediaMetadata>>;
}

impl TmdbSource for TmdbClient {
    async fn search_typed(
        &self,
        query: &str,
        kind: TmdbKind,
        year: Option<u32>,
        language: Option<&'static str>,
    ) -> Option<Vec<TmdbSuggestion>> {
        self.search_typed_in(query, kind, year, language).await
    }

    async fn lookup(&self, id: u64, kind: TmdbKind) -> Option<Option<MediaMetadata>> {
        self.try_lookup_with_kind(id, Some(kind)).await
    }
}

/// The strict SCENE match (T2) for a parsed release title, through the
/// persistent `tmdb_resolve_cache` (namespaced [`STRICT_CACHE_PREFIX`]).
/// Asks TMDB's typed search with the year filter and without it, so a ±1
/// movie year or a title buried under same-year noise is still among the
/// candidates, in English and in French (French releases carry the French
/// title, which TMDB returns as `title` only in `fr-FR`); only
/// [`strict_scene_match`] decides.
pub(crate) async fn strict_scene<S: TmdbSource>(
    pool: &SqlitePool,
    tmdb: &S,
    title: &str,
    kind: TmdbKind,
    year: Option<u32>,
) -> Result<Option<TmdbSuggestion>, Unreachable> {
    let query = iris_media::filename::series_key(title);
    if query.len() < 2 {
        return Ok(None);
    }
    let cache_key = match year {
        Some(y) => format!("{STRICT_CACHE_PREFIX}{query} {y}"),
        None => format!("{STRICT_CACHE_PREFIX}{query}"),
    };
    let kind_str = Some(kind.as_wire());
    if let Ok(Some(hit)) = tmdb_cache::get(
        pool,
        &cache_key,
        kind_str,
        chrono::Duration::days(MAX_AGE_DAYS),
    )
    .await
    {
        return Ok(from_entry(&hit, kind));
    }
    let mut candidates = Vec::new();
    for language in SEARCH_LANGUAGES {
        candidates.extend(
            tmdb.search_typed(&query, kind, year, language)
                .await
                .ok_or(Unreachable)?,
        );
        if year.is_some() {
            candidates.extend(
                tmdb.search_typed(&query, kind, None, language)
                    .await
                    .ok_or(Unreachable)?,
            );
        }
    }
    let matched = strict_scene_match(&candidates, title, kind, year).cloned();
    let entry = matched.as_ref().map_or_else(
        || ResolveEntry::not_found_at(chrono::Utc::now()),
        |m| ResolveEntry {
            tmdb_id: i64::try_from(m.tmdb_id).ok(),
            title: Some(m.title.clone()),
            year: m.year.map(i64::from),
            poster_path: m.poster_path.clone(),
            backdrop_path: None,
            overview: m.overview.clone(),
            fetched_at: chrono::Utc::now(),
        },
    );
    if let Err(e) = tmdb_cache::put(pool, &cache_key, kind_str, &entry).await {
        tracing::warn!(error = %e, cache_key, "tmdb trust: cache write failed");
    }
    Ok(matched)
}

fn from_entry(entry: &ResolveEntry, kind: TmdbKind) -> Option<TmdbSuggestion> {
    Some(TmdbSuggestion {
        tmdb_id: u64::try_from(entry.tmdb_id?).ok()?,
        kind,
        title: entry.title.clone().unwrap_or_default(),
        original_title: None,
        year: entry.year.and_then(|y| u32::try_from(y).ok()),
        poster_path: entry.poster_path.clone(),
        overview: entry.overview.clone(),
        vote_count: None,
    })
}

/// A full evaluation: the decision plus what it was made from.
#[derive(Debug, Clone)]
pub struct Evaluation {
    pub decision: Option<(u64, Trust)>,
    pub reason: Reason,
    pub scene: Option<TmdbSuggestion>,
    pub trackers: Vec<TrackerCheck>,
}

impl Evaluation {
    /// TMDB's title for the trusted id.
    pub fn trusted_title(&self) -> Option<String> {
        let (id, _) = self.decision?;
        if let Some(s) = self.scene.as_ref().filter(|s| s.tmdb_id == id) {
            return Some(s.title.clone());
        }
        self.trackers
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.meta.as_ref())
            .map(|m| m.title.clone())
    }

    /// The trusted match as a suggestion (poster, title, year).
    pub fn trusted_suggestion(&self) -> Option<TmdbSuggestion> {
        let (id, _) = self.decision?;
        if let Some(s) = self.scene.as_ref().filter(|s| s.tmdb_id == id) {
            return Some(s.clone());
        }
        let meta = self.trackers.iter().find(|t| t.id == id)?.meta.as_ref()?;
        Some(TmdbSuggestion {
            kind: meta.kind,
            tmdb_id: meta.tmdb_id,
            title: meta.title.clone(),
            original_title: None,
            year: meta.year,
            overview: meta.overview.clone(),
            poster_path: meta.poster_path.clone(),
            vote_count: None,
        })
    }
}

/// Evaluate a SCENE name (a collection's `display_title`, a search result's
/// release name) against TMDB, with the tracker ids vouching for it.
/// `kind` defaults to the one the name implies.
pub(crate) async fn evaluate<S: TmdbSource>(
    pool: &SqlitePool,
    tmdb: &S,
    scene_name: &str,
    kind: Option<TmdbKind>,
    tracker_ids: &[u64],
) -> Result<Evaluation, Unreachable> {
    let parsed = iris_media::filename::parse(scene_name);
    let kind = kind
        .or_else(|| parsed.as_ref().map(crate::tmdb_resolve::parsed_kind))
        .unwrap_or(TmdbKind::Movie);
    let year = parsed.as_ref().and_then(|p| p.year).map(u32::from);
    let mut trackers = Vec::with_capacity(tracker_ids.len());
    for &id in tracker_ids {
        let meta = tmdb.lookup(id, kind).await.ok_or(Unreachable)?;
        let plausible = meta
            .as_ref()
            .is_some_and(|m| tracker_id_plausible(m, kind, year));
        trackers.push(TrackerCheck {
            id,
            meta,
            plausible,
        });
    }
    let scene = match parsed.as_ref() {
        Some(p) => strict_scene(pool, tmdb, &p.title, kind, year).await?,
        None => None,
    };
    let (decision, reason) = if parsed.is_none() && trackers.is_empty() {
        (None, Reason::NoTitle)
    } else {
        decide(scene.as_ref().map(|s| s.tmdb_id), &trackers)
    };
    Ok(Evaluation {
        decision,
        reason,
        scene,
        trackers,
    })
}

/// The trusted TMDB match of one release, for display (search cards,
/// release preview, the resolve endpoint). `None` when nothing trusted
/// backs it — including when TMDB is unreachable.
pub async fn trusted_release_match(
    pool: &SqlitePool,
    tmdb: &TmdbClient,
    release_name: &str,
    kind: Option<TmdbKind>,
    tracker_id: Option<u64>,
) -> Option<TmdbSuggestion> {
    let trackers: Vec<u64> = tracker_id.into_iter().collect();
    evaluate(pool, tmdb, release_name, kind, &trackers)
        .await
        .ok()?
        .trusted_suggestion()
}

/// Re-evaluate a collection after a release joined it and store the
/// result under the conflict rule. Legacy rows are left alone.
pub(crate) async fn refresh_collection<S: TmdbSource>(
    pool: &SqlitePool,
    tmdb: &S,
    collection: &iris_db::collections::CollectionRow,
) {
    let trust = collection
        .tmdb_trust
        .as_deref()
        .and_then(Trust::from_stored);
    if collection.tmdb_id.is_some() && trust.is_none() {
        return;
    }
    let trackers = match iris_db::collections::tracker_tmdb_ids(pool, collection.id).await {
        Ok(ids) => ids
            .into_iter()
            .filter_map(|i| u64::try_from(i).ok())
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!(error = %e, collection_id = %collection.id, "tmdb trust: tracker ids read failed");
            return;
        }
    };
    let kind = TmdbKind::from_wire(&collection.kind);
    let Ok(eval) = evaluate(pool, tmdb, &collection.display_title, kind, &trackers).await else {
        tracing::debug!(collection_id = %collection.id, "tmdb trust: TMDB unreachable, left as is");
        return;
    };
    let Some((id, new_trust)) = eval.decision else {
        tracing::debug!(collection_id = %collection.id, reason = %eval.reason, "tmdb trust: no trusted match");
        return;
    };
    if !should_replace((collection.tmdb_id, trust), new_trust) {
        return;
    }
    let Ok(id) = i64::try_from(id) else { return };
    match iris_db::collections::set_trusted_tmdb(pool, collection.id, id, new_trust.as_str()).await
    {
        Ok(()) => tracing::info!(
            collection_id = %collection.id,
            tmdb_id = id,
            trust = new_trust.as_str(),
            reason = %eval.reason,
            "tmdb trust: collection match stored",
        ),
        Err(e) => {
            tracing::warn!(error = %e, collection_id = %collection.id, "tmdb trust: write failed");
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;

    use super::*;

    pub(crate) fn suggestion(id: u64, kind: TmdbKind, title: &str, year: u32) -> TmdbSuggestion {
        TmdbSuggestion {
            kind,
            tmdb_id: id,
            title: title.into(),
            original_title: None,
            year: Some(year),
            overview: None,
            poster_path: Some(format!("/{id}.jpg")),
            vote_count: None,
        }
    }

    pub(crate) fn meta(id: u64, kind: TmdbKind, title: &str, year: u32) -> MediaMetadata {
        MediaMetadata {
            kind,
            tmdb_id: id,
            title: title.into(),
            overview: None,
            year: Some(year),
            poster_path: Some(format!("/{id}.jpg")),
            backdrop_path: None,
            vote_score: None,
            vote_count: None,
            genres: Vec::new(),
            runtime_minutes: None,
            number_of_seasons: None,
            popularity: None,
            original_language: None,
            genre_ids: Vec::new(),
            release_date: None,
        }
    }

    /// Fixture TMDB: typed searches answer from `searches` (by lowercase
    /// query, any year — the strict rule filters), lookups from `ids`.
    #[derive(Default)]
    pub(crate) struct FakeTmdb {
        pub searches: HashMap<String, Vec<TmdbSuggestion>>,
        /// `fr-FR` answers, same keys; a query absent here answers as English.
        pub searches_fr: HashMap<String, Vec<TmdbSuggestion>>,
        pub ids: HashMap<u64, MediaMetadata>,
        pub offline: bool,
    }

    impl TmdbSource for FakeTmdb {
        #[expect(
            clippy::unused_async_trait_impl,
            reason = "fixture answers synchronously"
        )]
        async fn search_typed(
            &self,
            query: &str,
            kind: TmdbKind,
            _year: Option<u32>,
            language: Option<&'static str>,
        ) -> Option<Vec<TmdbSuggestion>> {
            if self.offline {
                return None;
            }
            let fr = language.and_then(|_| self.searches_fr.get(query));
            Some(
                fr.or_else(|| self.searches.get(query))
                    .map(|v| v.iter().filter(|s| s.kind == kind).cloned().collect())
                    .unwrap_or_default(),
            )
        }

        #[expect(
            clippy::unused_async_trait_impl,
            reason = "fixture answers synchronously"
        )]
        async fn lookup(&self, id: u64, _kind: TmdbKind) -> Option<Option<MediaMetadata>> {
            if self.offline {
                return None;
            }
            Some(self.ids.get(&id).cloned())
        }
    }

    #[test]
    fn title_key_folds_accents_apostrophes_and_ampersands() {
        assert_eq!(title_key("Amélie"), "amelie");
        assert_eq!(title_key("Ocean's Eleven"), "oceans eleven");
        assert_eq!(title_key("Ocean’s  Eleven!"), "oceans eleven");
        assert_eq!(title_key("Fast & Furious"), "fast and furious");
        assert_eq!(title_key("Fast&Furious"), "fast and furious");
        assert_eq!(title_key("Les Misérables: Œuvre"), "les miserables oeuvre");
        assert_eq!(
            title_key("Spider-Man: No Way Home"),
            "spider man no way home"
        );
        assert_eq!(title_key("ＡＢＣ"), "abc");
        assert_eq!(title_key("Part One"), "part one");
        assert_ne!(title_key("Part One"), title_key("Part 1"));
        assert_eq!(title_key("  ...  "), "");
    }

    #[test]
    fn strict_match_needs_the_exact_title_or_original_title() {
        let mut fr = suggestion(1, TmdbKind::Movie, "The Intouchables", 2011);
        fr.original_title = Some("Intouchables".into());
        let cands = vec![fr, suggestion(2, TmdbKind::Movie, "Untouchable", 2011)];
        assert_eq!(
            strict_scene_match(&cands, "Intouchables", TmdbKind::Movie, Some(2011))
                .map(|s| s.tmdb_id),
            Some(1)
        );
        assert_eq!(
            strict_scene_match(&cands, "The Intouchables", TmdbKind::Movie, Some(2011))
                .map(|s| s.tmdb_id),
            Some(1)
        );
        assert!(strict_scene_match(&cands, "Intouchable", TmdbKind::Movie, Some(2011)).is_none());
    }

    #[test]
    fn strict_match_checks_the_kind() {
        let cands = vec![suggestion(5, TmdbKind::Tv, "Dune", 2021)];
        assert!(strict_scene_match(&cands, "Dune", TmdbKind::Movie, Some(2021)).is_none());
        assert!(strict_scene_match(&cands, "Dune", TmdbKind::Tv, Some(2021)).is_some());
    }

    #[test]
    fn strict_match_year_rules() {
        let cands = vec![
            suggestion(438_631, TmdbKind::Movie, "Dune", 2021),
            suggestion(841, TmdbKind::Movie, "Dune", 1984),
        ];
        let pick = |y| strict_scene_match(&cands, "Dune", TmdbKind::Movie, y).map(|s| s.tmdb_id);
        assert_eq!(pick(Some(2021)), Some(438_631));
        assert_eq!(pick(Some(2022)), Some(438_631), "movies: ±1");
        assert_eq!(pick(Some(1985)), Some(841));
        assert_eq!(pick(Some(2019)), None, "beyond ±1 never closest-year");
        assert_eq!(pick(None), None, "two exact titles and no year: ambiguous");
        let one = vec![suggestion(9, TmdbKind::Movie, "Dune", 2021)];
        assert_eq!(
            strict_scene_match(&one, "Dune", TmdbKind::Movie, None).map(|s| s.tmdb_id),
            Some(9),
            "no year on the release: a unique exact title passes"
        );
        let tv = vec![suggestion(3, TmdbKind::Tv, "Doctor Who", 2005)];
        assert!(
            strict_scene_match(&tv, "Doctor Who", TmdbKind::Tv, Some(2006)).is_none(),
            "TV: equal"
        );
        assert!(strict_scene_match(&tv, "Doctor Who", TmdbKind::Tv, Some(2005)).is_some());
        let same_year_twins = vec![
            suggestion(1, TmdbKind::Movie, "Midnight", 2021),
            suggestion(2, TmdbKind::Movie, "Midnight", 2021),
        ];
        assert!(
            strict_scene_match(&same_year_twins, "Midnight", TmdbKind::Movie, Some(2021)).is_none()
        );
    }

    #[test]
    fn a_dominant_homonym_wins_a_tie_a_close_one_does_not() {
        let votes = |id, title, year, v| TmdbSuggestion {
            vote_count: Some(v),
            ..suggestion(id, TmdbKind::Movie, title, year)
        };
        let annihilation = vec![
            votes(300_668, "Annihilation", 2018, 6_900),
            votes(883_188, "Annihilation", 2018, 2),
        ];
        assert_eq!(
            strict_scene_match(&annihilation, "Annihilation", TmdbKind::Movie, Some(2018))
                .map(|s| s.tmdb_id),
            Some(300_668),
            "a same-year short with two votes doesn't make the film ambiguous"
        );
        let close = vec![
            votes(37_854, "One Piece", 1999, 5_000),
            votes(111_110, "One Piece", 2023, 2_100),
        ];
        assert!(
            strict_scene_match(&close, "One Piece", TmdbKind::Movie, None).is_none(),
            "two real titles under one name stay ambiguous"
        );
        let obscure = vec![
            votes(1, "Midnight", 2021, 30),
            votes(2, "Midnight", 2021, 0),
        ];
        assert!(
            strict_scene_match(&obscure, "Midnight", TmdbKind::Movie, Some(2021)).is_none(),
            "dominance needs a minimum of votes"
        );
        let twice = vec![
            votes(300_668, "Annihilation", 2018, 6_900),
            votes(300_668, "Annihilation", 2018, 6_900),
        ];
        assert!(
            strict_scene_match(&twice, "Annihilation", TmdbKind::Movie, Some(2018)).is_some(),
            "the same id from two searches is one candidate"
        );
    }

    #[test]
    fn a_french_elision_spelled_apart_still_matches() {
        let cands = vec![suggestion(
            783_570,
            TmdbKind::Movie,
            "C'est magnifique !",
            2022,
        )];
        assert!(
            strict_scene_match(&cands, "C Est Magnifique", TmdbKind::Movie, Some(2022)).is_some()
        );
        assert!(
            strict_scene_match(&cands, "Cest Magnifique", TmdbKind::Movie, Some(2022)).is_some()
        );
        assert!(strict_scene_match(&cands, "C Magnifique", TmdbKind::Movie, Some(2022)).is_none());
    }

    #[tokio::test]
    async fn the_french_title_counts_as_an_exact_title() {
        let pool = iris_db::test_support::migrated_pool().await;
        let english = suggestion(
            22,
            TmdbKind::Movie,
            "Pirates of the Caribbean: The Curse of the Black Pearl",
            2003,
        );
        let french = TmdbSuggestion {
            title: "Pirates des Caraïbes : La Malédiction du Black Pearl".into(),
            ..english.clone()
        };
        let query =
            iris_media::filename::series_key("Pirates des Caraibes La Malediction du Black Pearl");
        let tmdb = FakeTmdb {
            searches: HashMap::from([(query.clone(), vec![english])]),
            searches_fr: HashMap::from([(query, vec![french])]),
            ..FakeTmdb::default()
        };
        let hit = strict_scene(
            &pool,
            &tmdb,
            "Pirates des Caraibes La Malediction du Black Pearl",
            TmdbKind::Movie,
            Some(2003),
        )
        .await
        .unwrap();
        assert_eq!(hit.map(|s| s.tmdb_id), Some(22));
    }

    fn check(id: u64, plausible: bool) -> TrackerCheck {
        TrackerCheck {
            id,
            meta: None,
            plausible,
        }
    }

    #[test]
    fn decide_t1_agree_disagree_alone() {
        assert_eq!(
            decide(Some(7), &[check(7, true)]).0,
            Some((7, Trust::TrackerScene))
        );
        assert_eq!(
            decide(Some(7), &[check(7, false)]).0,
            Some((7, Trust::TrackerScene)),
            "agreement needs no plausibility check"
        );
        let (none, reason) = decide(Some(7), &[check(8, true)]);
        assert_eq!(none, None, "tracker and SCENE disagree: no cover");
        assert_eq!(
            reason,
            Reason::TrackerDisagrees {
                tracker: vec![8],
                scene: 7
            }
        );
        assert_eq!(decide(None, &[check(8, true)]).0, Some((8, Trust::Tracker)));
        assert_eq!(decide(None, &[check(8, false)]).0, None);
        assert_eq!(
            decide(None, &[check(8, true), check(9, true)]).1,
            Reason::TrackersConflict(vec![8, 9])
        );
        assert_eq!(decide(Some(7), &[]).0, Some((7, Trust::Scene)));
        assert_eq!(decide(None, &[]).1, Reason::NoSignal);
    }

    #[test]
    fn tracker_plausibility_is_kind_and_year_within_one() {
        let m = meta(1, TmdbKind::Movie, "X", 2020);
        assert!(tracker_id_plausible(&m, TmdbKind::Movie, Some(2021)));
        assert!(tracker_id_plausible(&m, TmdbKind::Movie, None));
        assert!(!tracker_id_plausible(&m, TmdbKind::Movie, Some(2022)));
        assert!(!tracker_id_plausible(&m, TmdbKind::Tv, Some(2020)));
    }

    #[test]
    fn conflict_rule_prefers_existing_unless_stronger() {
        use Trust::{Admin, Scene, Tracker, TrackerScene};
        assert!(should_replace((None, None), Tracker));
        assert!(
            !should_replace((Some(1), None), Admin),
            "legacy waits for --apply"
        );
        assert!(
            !should_replace((Some(1), Some(Scene)), Scene),
            "tie keeps the first writer"
        );
        assert!(!should_replace((Some(1), Some(Scene)), Tracker));
        assert!(should_replace((Some(1), Some(Scene)), TrackerScene));
        assert!(should_replace((Some(1), Some(Tracker)), Scene));
        assert!(!should_replace((Some(1), Some(Admin)), TrackerScene));
        for t in [Admin, TrackerScene, Scene, Tracker] {
            assert_eq!(Trust::from_stored(t.as_str()), Some(t));
        }
    }

    #[tokio::test]
    async fn evaluate_uses_the_cache_and_reports_offline() {
        let pool = iris_db::test_support::migrated_pool().await;
        let mut tmdb = FakeTmdb::default();
        tmdb.searches.insert(
            "dune".into(),
            vec![suggestion(438_631, TmdbKind::Movie, "Dune", 2021)],
        );
        let e = evaluate(&pool, &tmdb, "Dune (2021)", Some(TmdbKind::Movie), &[])
            .await
            .unwrap();
        assert_eq!(e.decision, Some((438_631, Trust::Scene)));
        tmdb.offline = true;
        let cached = evaluate(&pool, &tmdb, "Dune (2021)", Some(TmdbKind::Movie), &[])
            .await
            .unwrap();
        assert_eq!(
            cached.decision,
            Some((438_631, Trust::Scene)),
            "served from the strict cache"
        );
        assert_eq!(
            evaluate(&pool, &tmdb, "Arrival (2016)", Some(TmdbKind::Movie), &[])
                .await
                .unwrap_err(),
            Unreachable
        );
    }

    #[tokio::test]
    async fn ingest_refresh_spares_legacy_rows_and_follows_the_conflict_rule() {
        use iris_db::collections::{Kind, find_or_create, get};
        let pool = iris_db::test_support::migrated_pool().await;
        let mut tmdb = FakeTmdb::default();
        tmdb.searches.insert(
            "dune".into(),
            vec![suggestion(438_631, TmdbKind::Movie, "Dune", 2021)],
        );
        let fresh = find_or_create(&pool, "dune 2021", "Dune (2021)", Kind::Movie, false)
            .await
            .unwrap();
        refresh_collection(&pool, &tmdb, &fresh).await;
        let c = get(&pool, fresh.id).await.unwrap().unwrap();
        assert_eq!(
            (c.tmdb_id, c.tmdb_trust.as_deref()),
            (Some(438_631), Some("scene"))
        );

        let legacy = find_or_create(&pool, "dune 2020", "Dune (2021)", Kind::Movie, false)
            .await
            .unwrap();
        sqlx::query("UPDATE collections SET tmdb_id = 841 WHERE id = ?1")
            .bind(legacy.id)
            .execute(&pool)
            .await
            .unwrap();
        let legacy = get(&pool, legacy.id).await.unwrap().unwrap();
        refresh_collection(&pool, &tmdb, &legacy).await;
        let after = get(&pool, legacy.id).await.unwrap().unwrap();
        assert_eq!(
            (after.tmdb_id, after.tmdb_trust),
            (Some(841), None),
            "legacy untouched"
        );

        let mut other = FakeTmdb::default();
        other.searches.insert(
            "dune".into(),
            vec![suggestion(1, TmdbKind::Movie, "Dune", 2021)],
        );
        refresh_collection(&pool, &other, &c).await;
        assert_eq!(
            get(&pool, c.id).await.unwrap().unwrap().tmdb_id,
            Some(438_631),
            "an equally strong signal never overwrites a trusted id"
        );
    }
}
