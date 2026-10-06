//! Per-user playback preferences — preferred audio + subtitle *language*.
//!
//! - `GET /api/me/playback-preferences`  — the user's preferred languages.
//! - `PUT /api/me/playback-preferences`  — save them (client sends the full
//!   current state). `subtitle_language: "off"` means "no subtitles".
//! - Both take an optional `collection_id`: one series' own choice ("kept
//!   for the whole series"), falling back to the account-wide one. Without
//!   it, the account-wide preference, as shipped clients expect.
//!
//! Separate from `/api/me/preferences` (the reco onboarding prefs) on purpose:
//! that endpoint full-replaces its row, so adding fields there would let a
//! shipped 0.5.x client reset them. A dedicated endpoint old clients never
//! call is breakage-proof.

use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::error::ApiResult;
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(get_prefs).put(put_prefs))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PlaybackPrefsResponse {
    /// Preferred audio language (ISO 639-1 / BCP-47), or null = no preference.
    audio_language: Option<String>,
    /// Preferred subtitle language, `"off"` for disabled, or null = no
    /// preference.
    subtitle_language: Option<String>,
    /// `true` when these come from the series' own choice rather than the
    /// account-wide one. Additive — always `false` without `collection_id`.
    #[serde(default)]
    for_collection: bool,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct PrefsScope {
    /// The series (collection) to read the choice of.
    collection_id: Option<Uuid>,
}

#[utoipa::path(
    get,
    path = "/api/me/playback-preferences",
    operation_id = "get_playback_preferences",
    params(PrefsScope),
    responses((status = 200, description = "The caller's preferred audio + subtitle languages", body = PlaybackPrefsResponse)),
    tag = "preferences",
)]
pub(crate) async fn get_prefs(
    State(state): State<AppState>,
    user: AuthUser,
    Query(scope): Query<PrefsScope>,
) -> ApiResult<Json<PlaybackPrefsResponse>> {
    let (p, for_collection) = match scope.collection_id {
        Some(c) => {
            iris_db::playback_preferences::get_for_collection(state.db(), user.id, c).await?
        }
        None => (
            iris_db::playback_preferences::get(state.db(), user.id).await?,
            false,
        ),
    };
    Ok(Json(PlaybackPrefsResponse {
        audio_language: p.audio_language,
        subtitle_language: p.subtitle_language,
        for_collection,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct UpdatePlaybackPrefs {
    #[serde(default)]
    audio_language: Option<String>,
    #[serde(default)]
    subtitle_language: Option<String>,
    /// Save as this series' own choice instead of the account-wide one.
    #[serde(default)]
    collection_id: Option<Uuid>,
}

/// Normalise a language token: trim + lowercase; empty → `None`. Any non-empty
/// value is accepted (track languages are arbitrary BCP-47 codes, plus the
/// `"off"` subtitle sentinel) — we never hardcode a vocabulary here.
fn norm(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
}

#[utoipa::path(
    put,
    path = "/api/me/playback-preferences",
    operation_id = "set_playback_preferences",
    request_body = UpdatePlaybackPrefs,
    responses((status = 204, description = "Preferences saved")),
    tag = "preferences",
)]
pub(crate) async fn put_prefs(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UpdatePlaybackPrefs>,
) -> ApiResult<StatusCode> {
    let prefs = iris_db::playback_preferences::PlaybackPreferences {
        audio_language: norm(body.audio_language),
        subtitle_language: norm(body.subtitle_language),
    };
    match body.collection_id {
        Some(c) => {
            iris_db::playback_preferences::set_for_collection(state.db(), user.id, c, &prefs)
                .await?;
        }
        None => iris_db::playback_preferences::set(state.db(), user.id, &prefs).await?,
    }
    Ok(StatusCode::NO_CONTENT)
}
