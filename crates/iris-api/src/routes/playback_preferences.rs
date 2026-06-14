//! Per-user playback preferences — preferred audio + subtitle *language*.
//!
//! - `GET /api/me/playback-preferences`  — the user's preferred languages.
//! - `PUT /api/me/playback-preferences`  — save them (client sends the full
//!   current state). `subtitle_language: "off"` means "no subtitles".
//!
//! Separate from `/api/me/preferences` (the reco onboarding prefs) on purpose:
//! that endpoint full-replaces its row, so adding fields there would let a
//! shipped 0.5.x client reset them. A dedicated endpoint old clients never
//! call is breakage-proof.

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

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
}

#[utoipa::path(
    get,
    path = "/api/me/playback-preferences",
    operation_id = "get_playback_preferences",
    responses((status = 200, description = "The caller's preferred audio + subtitle languages", body = PlaybackPrefsResponse)),
    tag = "preferences",
)]
pub(crate) async fn get_prefs(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<PlaybackPrefsResponse>> {
    let p = iris_db::playback_preferences::get(state.db(), user.id).await?;
    Ok(Json(PlaybackPrefsResponse {
        audio_language: p.audio_language,
        subtitle_language: p.subtitle_language,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct UpdatePlaybackPrefs {
    #[serde(default)]
    audio_language: Option<String>,
    #[serde(default)]
    subtitle_language: Option<String>,
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
    iris_db::playback_preferences::set(state.db(), user.id, &prefs).await?;
    Ok(StatusCode::NO_CONTENT)
}
