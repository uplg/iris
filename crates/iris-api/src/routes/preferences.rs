//! Per-user recommendation preferences + the genre taxonomy that feeds
//! the onboarding picker.
//!
//! - `GET/PUT /api/me/preferences` — read / save the authenticated
//!   user's languages, genres, anime toggle and onboarding flag.
//! - `GET /api/genres` — the merged movie+TV genre list (deduped by id)
//!   the onboarding dialog renders as selectable chips.

use std::collections::HashSet;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;
use crate::tmdb::TmdbKind;

/// User-selectable languages as `(value, label)`. Single server-side
/// source of truth: `GET /api/languages` serves this list and
/// `PUT /api/me/preferences` validates against it, so adding a language
/// is a backend-only change — clients fetch the list and never hardcode
/// it (no web redeploy / APK release just to add a language). `value`
/// must match the `iris_media::filename::Language` wire vocabulary (what
/// `detect_language` / `Language::satisfies` produce); "multi"/"unknown"
/// are derived from titles, never chosen as a preference.
const LANGUAGE_OPTIONS: [(&str, &str); 2] = [("french", "French"), ("english", "English")];

/// Preferences router — nested under `/api/me/preferences`.
pub fn router() -> Router<AppState> {
    Router::new().route("/", get(get_preferences).put(put_preferences))
}

/// Genre-taxonomy router — nested at `/api/genres`.
pub fn genres_router() -> Router<AppState> {
    Router::new().route("/", get(genres))
}

/// Language-options router — nested at `/api/languages`.
pub fn languages_router() -> Router<AppState> {
    Router::new().route("/", get(languages))
}

#[derive(Debug, Serialize)]
struct PreferencesResponse {
    languages: Vec<String>,
    genres: Vec<i64>,
    include_anime: bool,
    onboarding_completed: bool,
}

impl From<iris_db::preferences::UserPreferences> for PreferencesResponse {
    fn from(p: iris_db::preferences::UserPreferences) -> Self {
        Self {
            languages: p.languages,
            genres: p.genres,
            include_anime: p.include_anime,
            onboarding_completed: p.onboarding_completed,
        }
    }
}

async fn get_preferences(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<PreferencesResponse>> {
    let prefs = iris_db::preferences::get(state.db(), user.id).await?;
    Ok(Json(prefs.into()))
}

#[derive(Debug, Deserialize)]
struct UpdatePreferencesRequest {
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    genres: Vec<i64>,
    #[serde(default)]
    include_anime: bool,
    #[serde(default)]
    onboarding_completed: bool,
}

async fn put_preferences(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UpdatePreferencesRequest>,
) -> ApiResult<Json<PreferencesResponse>> {
    // Normalise the language list: validate against the allowed
    // vocabulary, lowercase, and de-dupe while preserving the user's
    // ordering (most-preferred first). An unknown language is a client
    // bug, so reject rather than silently drop it.
    let mut languages: Vec<String> = Vec::with_capacity(body.languages.len());
    for lang in &body.languages {
        let l = lang.trim().to_lowercase();
        if !LANGUAGE_OPTIONS.iter().any(|(value, _)| *value == l.as_str()) {
            return Err(ApiError::BadRequest(format!("unknown language: {lang}")));
        }
        if !languages.contains(&l) {
            languages.push(l);
        }
    }
    // Genre ids: keep positive, distinct, order-preserving.
    let mut genres: Vec<i64> = Vec::with_capacity(body.genres.len());
    for &g in &body.genres {
        if g > 0 && !genres.contains(&g) {
            genres.push(g);
        }
    }
    let update = iris_db::preferences::PreferencesUpdate {
        languages,
        genres,
        include_anime: body.include_anime,
        onboarding_completed: body.onboarding_completed,
    };
    let prefs = iris_db::preferences::upsert(state.db(), user.id, &update).await?;
    // Prefs drive the "For You" shelves — drop the cached build so the
    // next request reflects the change immediately.
    crate::reco::invalidate(user.id);
    Ok(Json(prefs.into()))
}

#[derive(Debug, Serialize)]
struct GenreOption {
    id: i64,
    name: String,
}

#[derive(Debug, Serialize)]
struct GenresResponse {
    genres: Vec<GenreOption>,
}

/// Merged movie + TV genre taxonomy, deduped by id and sorted by name.
/// Anime is deliberately NOT in this list: it is a distinct category —
/// NOT TMDB's "Animation" genre (id 16) — backed by its own `AniList`
/// pipeline and driven by the separate `include_anime` preference, never
/// a TMDB genre id. Clients surface it as its own selectable chip.
/// Returns an empty list when TMDB is unconfigured rather than failing
/// the onboarding flow.
async fn genres(
    State(state): State<AppState>,
    _user: AuthUser,
) -> ApiResult<Json<GenresResponse>> {
    let Some(tmdb) = state.tmdb() else {
        return Ok(Json(GenresResponse { genres: Vec::new() }));
    };
    let mut seen: HashSet<u32> = HashSet::new();
    let mut out: Vec<GenreOption> = Vec::new();
    for kind in [TmdbKind::Movie, TmdbKind::Tv] {
        for g in tmdb.genre_list(kind).await {
            if seen.insert(g.id) {
                out.push(GenreOption {
                    id: i64::from(g.id),
                    name: g.name,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(GenresResponse { genres: out }))
}

#[derive(Debug, Serialize)]
struct LanguageOption {
    value: String,
    label: String,
}

#[derive(Debug, Serialize)]
struct LanguagesResponse {
    languages: Vec<LanguageOption>,
}

/// The user-selectable language vocabulary (value + display label).
/// Served so clients render the onboarding language chips from the server
/// instead of a hardcoded list — adding a language never requires a
/// client redeploy. Sourced from the same `LANGUAGE_OPTIONS` the PUT
/// handler validates against, so the list and the validation can't drift.
async fn languages(_user: AuthUser) -> Json<LanguagesResponse> {
    let languages = LANGUAGE_OPTIONS
        .iter()
        .map(|(value, label)| LanguageOption {
            value: (*value).to_string(),
            label: (*label).to_string(),
        })
        .collect();
    Json(LanguagesResponse { languages })
}
