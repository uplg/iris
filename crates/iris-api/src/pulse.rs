//! Discovery pulse: what the world is watching right now, joined against
//! what our trackers can actually serve.
//!
//! Every cycle:
//!   1. snapshot the external lists into `pulse_signals` — TMDB trending
//!      (week), French digital releases (last weeks), series on the air, one
//!      TMDB discover per curated mood, and SIMKL "watched today";
//!   2. join a budgeted batch of those titles against the catalogue trackers
//!      by **searching** them (English title + year, then the French title),
//!      verify the best hit resolves back to the same TMDB id, and upsert it
//!      through the freshness path (`availability='available'`). A miss is
//!      only recorded in `pulse_checks`, never in `catalog_items`.
//!
//! Titles a scheduler confirmed recently, or that were just checked, are
//! skipped, so a cycle spends its budget on what's new on the lists.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::Utc;
use iris_config::DiscoveryConfig;
use iris_core::search::{MediaKind, SearchQuery, SearchResult, SortField, SortOrder};
use iris_db::SqlitePool;
use iris_db::pulse::NewSignal;
use iris_media::filename::Language;
use iris_providers::ProviderRegistry;

use crate::anilist::AniListClient;
use crate::freshness_scheduler::upsert_window_rows;
use crate::ranking::candidate_of;
use crate::simkl::SimklClient;
use crate::tmdb::{DiscoverFilter, MediaMetadata, TmdbClient, TmdbKind};

const CYCLE: Duration = Duration::from_hours(3);
/// After boot, let the freshness scheduler and the providers settle first.
const WARMUP: Duration = Duration::from_secs(90);
/// Titles searched per cycle — each one is a search on every tracker.
const JOIN_BUDGET: usize = 60;
/// Pause between two joined titles, so a cycle trickles instead of bursting
/// (some trackers rate-limit to ~10 requests a minute).
const JOIN_SPACING: Duration = Duration::from_secs(10);
/// A title found less than this ago is not searched again.
const HIT_TTL: chrono::Duration = chrono::Duration::hours(12);
/// A title missed less than this ago is not searched again.
const MISS_TTL: chrono::Duration = chrono::Duration::hours(24);
/// A catalogue row refreshed within this needs no join.
const AVAILABLE_FRESH: chrono::Duration = chrono::Duration::days(3);
/// Snapshots / join records older than this are dropped.
const RETENTION: chrono::Duration = chrono::Duration::days(3);
/// "Just out on streaming" looks this far back.
const DIGITAL_WINDOW_DAYS: i64 = 45;
/// Search hits verified against TMDB before giving up on a title.
const VERIFY_ATTEMPTS: usize = 3;
/// SIMKL entries kept per kind.
const SIMKL_TOP: usize = 60;
/// Pages fetched per TMDB list (20 titles each).
const LIST_PAGES: u32 = 2;

/// Join order across lists when ranks tie: the broad pulse first.
const LIST_PRIORITY: [&str; 4] = ["trending", "simkl", "digital", "on_air"];

/// A curated mood: an opaque id, its label and, per kind, the TMDB genre
/// rule defining it. A kind without a rule doesn't offer the mood.
pub(crate) struct Mood {
    pub id: &'static str,
    pub label: &'static str,
    pub movie: Option<GenreRule>,
    pub tv: Option<GenreRule>,
}

/// Any of `any` (or no constraint when empty), none of `none`.
#[derive(Clone, Copy)]
pub(crate) struct GenreRule {
    pub any: &'static [i64],
    pub none: &'static [i64],
}

impl GenreRule {
    pub fn matches(self, genres: &[i64]) -> bool {
        (self.any.is_empty() || genres.iter().any(|g| self.any.contains(g)))
            && !genres.iter().any(|g| self.none.contains(g))
    }
}

impl Mood {
    pub fn rule(&self, kind: TmdbKind) -> Option<GenreRule> {
        match kind {
            TmdbKind::Movie => self.movie,
            TmdbKind::Tv => self.tv,
        }
    }

    pub fn list(&self) -> String {
        format!("mood:{}", self.id)
    }
}

const fn rule(any: &'static [i64], none: &'static [i64]) -> GenreRule {
    GenreRule { any, none }
}

/// TMDB genre ids — movie: 27 horror, 53 thriller, 35 comedy, 10751 family,
/// 10402 music, 28 action, 9648 mystery, 10749 romance, 878 sci-fi,
/// 16 animation, 99 documentary, 36 history, 80 crime, 10752 war, 18 drama.
/// TV: 10759 action & adventure, 10765 sci-fi & fantasy, 10764 reality,
/// 10762 kids, 10768 war & politics. TV has no horror / romance genre.
pub(crate) const MOODS: &[Mood] = &[
    Mood {
        id: "chills",
        label: "Chills",
        movie: Some(rule(&[27, 53], &[16, 10751, 35])),
        tv: None,
    },
    Mood {
        id: "feel-good",
        label: "Feel-good",
        movie: Some(rule(&[10751, 10402], &[27, 53, 80, 10752])),
        tv: Some(rule(&[10751, 10764], &[80, 10768, 16])),
    },
    Mood {
        id: "action",
        label: "Pure action",
        movie: Some(rule(&[28], &[16])),
        tv: Some(rule(&[10759], &[16])),
    },
    Mood {
        id: "mind-bending",
        label: "Mind-bending",
        movie: Some(rule(&[9648], &[16, 10751])),
        tv: Some(rule(&[9648], &[16, 10762])),
    },
    Mood {
        id: "laughs",
        label: "Laughs",
        movie: Some(rule(&[35], &[16, 10751, 27])),
        tv: Some(rule(&[35], &[16, 10762])),
    },
    Mood {
        id: "love",
        label: "Love story",
        movie: Some(rule(&[10749], &[16, 27])),
        tv: None,
    },
    Mood {
        id: "space",
        label: "Space & sci-fi",
        movie: Some(rule(&[878], &[16])),
        tv: Some(rule(&[10765], &[16])),
    },
    Mood {
        id: "animation",
        label: "Animation",
        movie: Some(rule(&[16], &[])),
        tv: Some(rule(&[16], &[])),
    },
    Mood {
        id: "true-stories",
        label: "True stories",
        movie: Some(rule(&[99, 36], &[])),
        tv: Some(rule(&[99], &[])),
    },
];

struct Deps {
    pool: SqlitePool,
    tmdb: TmdbClient,
    simkl: Option<SimklClient>,
    anilist: Option<AniListClient>,
    providers: ProviderRegistry,
    max_content_age_years: i64,
}

pub fn spawn(
    pool: SqlitePool,
    tmdb: TmdbClient,
    providers: ProviderRegistry,
    cfg: &DiscoveryConfig,
    anilist: Option<AniListClient>,
) {
    if providers.catalog_ids().is_empty() {
        tracing::info!("pulse scheduler: no catalogue providers; not starting");
        return;
    }
    let simkl = SimklClient::new()
        .inspect_err(|e| tracing::warn!(error = %e, "simkl init failed; pulse runs on TMDB only"))
        .ok();
    let deps = Deps {
        pool,
        tmdb,
        simkl,
        anilist,
        providers,
        max_content_age_years: cfg.max_content_age_years,
    };
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        let mut ticker = tokio::time::interval(CYCLE);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            crate::supervise::tick("pulse", run_cycle(&deps)).await;
        }
    });
    tracing::info!("pulse scheduler started");
}

async fn run_cycle(deps: &Deps) {
    refresh_lists(deps).await;
    let joined = join_batch(deps).await;
    let pruned = iris_db::pulse::prune(&deps.pool, Utc::now() - RETENTION)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "pulse: prune failed");
            0
        });
    tracing::info!(joined, pruned, "pulse: cycle complete");
}

fn signals_of(metas: Vec<MediaMetadata>) -> Vec<NewSignal> {
    metas
        .into_iter()
        .filter_map(|m| {
            Some(NewSignal {
                tmdb_id: i64::try_from(m.tmdb_id).ok()?,
                watched: None,
                title: m.title,
                release_date: m.release_date,
            })
        })
        .collect()
}

async fn store(deps: &Deps, list: &str, kind: TmdbKind, entries: &[NewSignal]) {
    let kind = kind.as_wire();
    if let Err(e) = iris_db::pulse::replace_list(&deps.pool, list, kind, entries).await {
        tracing::warn!(error = %e, list, kind, "pulse: storing list failed");
    }
}

async fn paged<F, Fut>(fetch: F) -> Vec<MediaMetadata>
where
    F: Fn(u32) -> Fut,
    Fut: Future<Output = Vec<MediaMetadata>>,
{
    let mut out = Vec::new();
    for page in 1..=LIST_PAGES {
        out.extend(fetch(page).await);
    }
    out
}

async fn refresh_lists(deps: &Deps) {
    let tmdb = &deps.tmdb;
    let today = Utc::now().date_naive();
    for kind in [TmdbKind::Movie, TmdbKind::Tv] {
        let trending = paged(|p| tmdb.trending(kind, p)).await;
        store(deps, "trending", kind, &signals_of(trending)).await;

        if let Some(simkl) = &deps.simkl {
            let entries: Vec<NewSignal> = simkl
                .trending_today(kind)
                .await
                .into_iter()
                .take(SIMKL_TOP)
                .map(|t| NewSignal {
                    tmdb_id: t.tmdb_id,
                    watched: t.watched,
                    title: t.title,
                    release_date: None,
                })
                .collect();
            store(deps, "simkl", kind, &entries).await;
        }

        for mood in MOODS {
            let Some(rule) = mood.rule(kind) else {
                continue;
            };
            let filter = mood_filter(rule, kind, today);
            let found = paged(|p| tmdb.discover(kind, &filter, p)).await;
            store(deps, &mood.list(), kind, &signals_of(found)).await;
        }
    }

    let since = today - chrono::Duration::days(DIGITAL_WINDOW_DAYS);
    let digital = paged(|p| tmdb.digital_releases_fr(since, today, p)).await;
    store(deps, "digital", TmdbKind::Movie, &signals_of(digital)).await;

    let on_air = paged(|p| tmdb.on_the_air(p)).await;
    store(deps, "on_air", TmdbKind::Tv, &signals_of(on_air)).await;
}

/// A mood's TMDB discover query: its genres within a recency window (moods
/// are about what's around now, not the all-time canon).
fn mood_filter(rule: GenreRule, kind: TmdbKind, today: chrono::NaiveDate) -> DiscoverFilter {
    let join =
        |ids: &[i64], sep: &str| ids.iter().map(i64::to_string).collect::<Vec<_>>().join(sep);
    let (years, min_votes) = match kind {
        TmdbKind::Movie => (3, 30),
        TmdbKind::Tv => (5, 15),
    };
    DiscoverFilter {
        // `|` = OR in TMDB's syntax, `,` = AND.
        with_genres: join(rule.any, "|"),
        without_genres: join(rule.none, ","),
        since: today - chrono::Duration::days(365 * years),
        min_votes,
    }
}

struct Target {
    tmdb_id: i64,
    kind: MediaKind,
    list: String,
    local_title: String,
}

/// Order every listed title for joining — hottest rank first, ties broken
/// by list — minus what needs no search right now.
async fn join_queue(pool: &SqlitePool) -> Result<Vec<Target>, sqlx::Error> {
    let now = Utc::now();
    let mut skip: HashSet<(i64, String)> =
        iris_db::catalog::fresh_available_keys(pool, now - AVAILABLE_FRESH)
            .await?
            .into_iter()
            .collect();
    skip.extend(iris_db::pulse::recently_checked(pool, now - HIT_TTL, now - MISS_TTL).await?);

    let mut signals = iris_db::pulse::all_signals(pool).await?;
    let priority = |list: &str| {
        LIST_PRIORITY
            .iter()
            .position(|l| *l == list)
            .unwrap_or(LIST_PRIORITY.len())
    };
    signals.sort_by_key(|s| (s.rank, priority(&s.list)));

    let mut queued: HashSet<(i64, String)> = HashSet::new();
    let mut out = Vec::new();
    for s in signals {
        let key = (s.tmdb_id, s.kind.clone());
        if skip.contains(&key) || !queued.insert(key) {
            continue;
        }
        let Some(kind) = MediaKind::from_wire(&s.kind) else {
            continue;
        };
        out.push(Target {
            tmdb_id: s.tmdb_id,
            kind,
            list: s.list,
            local_title: s.title,
        });
    }
    Ok(out)
}

async fn join_batch(deps: &Deps) -> usize {
    let queue = match join_queue(&deps.pool).await {
        Ok(q) => q,
        Err(e) => {
            tracing::warn!(error = %e, "pulse: building the join queue failed");
            return 0;
        }
    };
    let mut joined = 0;
    for (i, target) in queue.iter().take(JOIN_BUDGET).enumerate() {
        if i > 0 {
            tokio::time::sleep(JOIN_SPACING).await;
        }
        let found = join_one(deps, target).await;
        joined += usize::from(found);
        if let Err(e) =
            iris_db::pulse::record_check(&deps.pool, target.tmdb_id, target.kind.as_wire(), found)
                .await
        {
            tracing::warn!(error = %e, "pulse: recording a join failed");
        }
    }
    joined
}

/// Search the trackers for one listed title and upsert its best verified
/// release. `true` when a row was written.
async fn join_one(deps: &Deps, target: &Target) -> bool {
    let tk = TmdbKind::from(target.kind);
    let Ok(id) = u64::try_from(target.tmdb_id) else {
        return false;
    };
    let Some(meta) = deps.tmdb.lookup_with_kind(id, Some(tk)).await else {
        return false;
    };
    // `lookup_with_kind` falls back to the other kind; a same-numbered title
    // of the other kind is a different work.
    if meta.kind != tk {
        return false;
    }
    let mut titles = vec![meta.title.clone()];
    if !target.local_title.is_empty() && title_key(&target.local_title) != title_key(&meta.title) {
        titles.push(target.local_title.clone());
    }
    let keys: HashSet<String> = titles.iter().map(|t| title_key(t)).collect();

    for title in &titles {
        let agg = deps
            .providers
            .search_catalog(&search_query(title, target.kind, meta.year))
            .await;
        let mut candidates: Vec<(SearchResult, Language)> = agg
            .results
            .into_iter()
            .filter(|r| {
                r.is_probably_video()
                    && r.seeders != Some(0)
                    && release_matches(&r.title, &keys, target.kind, meta.year)
            })
            .map(|r| {
                let lang = crate::ranking::resolve_language(&r, &deps.providers);
                (r, lang)
            })
            .collect();
        candidates.sort_by(|(a, la), (b, lb)| {
            iris_core::ranking::recommended_cmp(
                &candidate_of(a, *la == Language::Multi),
                &candidate_of(b, *lb == Language::Multi),
            )
        });
        for (release, lang) in candidates.into_iter().take(VERIFY_ATTEMPTS) {
            let verified = crate::tmdb_resolve::resolve_release_name(
                &deps.pool,
                &deps.tmdb,
                &release.title,
                Some(tk),
            )
            .await
            .is_some_and(|r| r.tmdb_id == id);
            if !verified {
                continue;
            }
            let best = HashMap::from([(target.tmdb_id, (release, lang))]);
            let source = format!("pulse:{}", target.list);
            return upsert_window_rows(
                &deps.pool,
                &deps.tmdb,
                deps.anilist.as_ref(),
                &source,
                target.kind,
                best,
                deps.max_content_age_years,
            )
            .await
                > 0;
        }
    }
    false
}

fn search_query(title: &str, kind: MediaKind, year: Option<u32>) -> SearchQuery {
    let q = match (kind, year) {
        (MediaKind::Movie, Some(y)) => format!("{title} {y}"),
        _ => title.to_string(),
    };
    SearchQuery {
        q,
        page: Some(1),
        limit: Some(100),
        sort_by: Some(SortField::Seeders),
        order: Some(SortOrder::Desc),
        kind: Some(kind),
        ..SearchQuery::default()
    }
}

/// Does a release name carry this title? SCENE title compared on its folded
/// key; a movie must also be a non-episode release within a year of TMDB's.
fn release_matches(name: &str, keys: &HashSet<String>, kind: MediaKind, year: Option<u32>) -> bool {
    let Some(parsed) = iris_media::filename::parse(name) else {
        return false;
    };
    if !keys.contains(&title_key(&parsed.title)) {
        return false;
    }
    match kind {
        MediaKind::Tv => true,
        MediaKind::Movie => {
            !parsed.is_tv()
                && match (parsed.year, year) {
                    (Some(p), Some(y)) => u32::from(p).abs_diff(y) <= 1,
                    _ => true,
                }
        }
    }
}

/// `series_key` with French / Latin accents folded — SCENE names are ASCII
/// (`Amelie`), TMDB titles aren't (`Amélie`).
fn title_key(title: &str) -> String {
    let folded: String = title
        .chars()
        .flat_map(|c| {
            let base = match c {
                'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => "a",
                'À' | 'Â' | 'Ä' | 'Á' | 'Ã' | 'Å' => "A",
                'é' | 'è' | 'ê' | 'ë' => "e",
                'É' | 'È' | 'Ê' | 'Ë' => "E",
                'î' | 'ï' | 'í' | 'ì' => "i",
                'Î' | 'Ï' | 'Í' | 'Ì' => "I",
                'ô' | 'ö' | 'ó' | 'ò' | 'õ' | 'ø' => "o",
                'Ô' | 'Ö' | 'Ó' | 'Ò' | 'Õ' | 'Ø' => "O",
                'ù' | 'û' | 'ü' | 'ú' => "u",
                'Ù' | 'Û' | 'Ü' | 'Ú' => "U",
                'ç' => "c",
                'Ç' => "C",
                'ñ' => "n",
                'Ñ' => "N",
                'œ' => "oe",
                'Œ' => "OE",
                'æ' => "ae",
                'Æ' => "AE",
                '&' => " and ",
                _ => "",
            };
            if base.is_empty() {
                vec![c]
            } else {
                base.chars().collect()
            }
        })
        .collect();
    iris_media::filename::series_key(&folded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(titles: &[&str]) -> HashSet<String> {
        titles.iter().map(|t| title_key(t)).collect()
    }

    #[test]
    fn title_keys_fold_accents_and_punctuation() {
        assert_eq!(
            title_key("Le Fabuleux Destin d'Amélie Poulain"),
            "le fabuleux destin d amelie poulain"
        );
        assert_eq!(
            title_key("Spider-Man: Across the Spider-Verse"),
            "spider man across the spider verse"
        );
        assert_eq!(title_key("Fast & Furious"), title_key("Fast and Furious"));
    }

    #[test]
    fn movie_releases_match_title_and_year() {
        let k = keys(&["Dune: Part Two", "Dune : Deuxième partie"]);
        let y = Some(2024);
        assert!(release_matches(
            "Dune.Part.Two.2024.MULTi.1080p.WEB.x264-GRP",
            &k,
            MediaKind::Movie,
            y
        ));
        assert!(release_matches(
            "Dune.Deuxieme.Partie.2024.FRENCH.1080p.WEB.x264-GRP",
            &k,
            MediaKind::Movie,
            y
        ));
        assert!(
            !release_matches(
                "Dune.Part.Two.2021.1080p.WEB.x264-GRP",
                &k,
                MediaKind::Movie,
                y
            ),
            "wrong year"
        );
        assert!(!release_matches(
            "Dune.Prophecy.S01E01.1080p.WEB.x264-GRP",
            &k,
            MediaKind::Movie,
            y
        ));
        assert!(
            !release_matches(
                "Dune.Part.Two.S01E01.1080p.WEB.x264-GRP",
                &k,
                MediaKind::Movie,
                y
            ),
            "episode, not a film"
        );
    }

    #[test]
    fn series_releases_match_on_title() {
        let k = keys(&["Ted Lasso"]);
        assert!(release_matches(
            "Ted.Lasso.S04E02.MULTi.1080p.WEB.H264-GRP",
            &k,
            MediaKind::Tv,
            Some(2020)
        ));
        assert!(release_matches(
            "Ted.Lasso.S01.1080p.WEB.H264-GRP",
            &k,
            MediaKind::Tv,
            Some(2020)
        ));
        assert!(!release_matches(
            "Ted.2012.1080p.BluRay.x264-GRP",
            &k,
            MediaKind::Tv,
            Some(2020)
        ));
    }

    #[test]
    fn mood_rules_and_filters() {
        let chills = MOODS.iter().find(|m| m.id == "chills").unwrap();
        let rule = chills.rule(TmdbKind::Movie).unwrap();
        assert!(rule.matches(&[27, 18]));
        assert!(!rule.matches(&[27, 35]), "horror-comedy is not chills");
        assert!(!rule.matches(&[18]));
        assert!(chills.rule(TmdbKind::Tv).is_none());

        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let f = mood_filter(rule, TmdbKind::Movie, today);
        assert_eq!(f.with_genres, "27|53");
        assert_eq!(f.without_genres, "16,10751,35");

        let ids: HashSet<&str> = MOODS.iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), MOODS.len(), "mood ids are unique");
    }
}
