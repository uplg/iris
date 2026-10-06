//! Per-user playback preferences — preferred audio + subtitle *language*.
//!
//! - `GET /api/me/playback-preferences`  — the user's preferred languages.
//! - `PUT /api/me/playback-preferences`  — save them (replaces the row).
//!   `subtitle_language: "off"` means "no subtitles".
//! - Both take an optional `collection_id`: one title's own choice ("kept
//!   for the whole series" / "for this film"). Read, each field the title
//!   left NULL inherits the account-wide one; written, a null field is "no
//!   choice of its own". Without it, the account-wide preference, as shipped
//!   clients expect.
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
    /// `true` when the title holds a choice of its own for at least one
    /// field. Additive — always `false` without `collection_id`.
    #[serde(default)]
    for_collection: bool,
    /// `audio_language` is the title's own choice, not the account's.
    /// Additive — always `false` without `collection_id`.
    #[serde(default)]
    audio_for_collection: bool,
    /// `subtitle_language` is the title's own choice, not the account's.
    /// Additive — always `false` without `collection_id`.
    #[serde(default)]
    subtitle_for_collection: bool,
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
    let p = match scope.collection_id {
        Some(c) => {
            iris_db::playback_preferences::get_for_collection(state.db(), user.id, c).await?
        }
        None => iris_db::playback_preferences::CollectionPreferences {
            merged: iris_db::playback_preferences::get(state.db(), user.id).await?,
            own: iris_db::playback_preferences::PlaybackPreferences::default(),
        },
    };
    Ok(Json(PlaybackPrefsResponse {
        for_collection: p.for_collection(),
        audio_for_collection: p.own.audio_language.is_some(),
        subtitle_for_collection: p.own.subtitle_language.is_some(),
        audio_language: p.merged.audio_language,
        subtitle_language: p.merged.subtitle_language,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct UpdatePlaybackPrefs {
    #[serde(default)]
    audio_language: Option<String>,
    #[serde(default)]
    subtitle_language: Option<String>,
    /// Save as this title's own choice instead of the account-wide one: send
    /// only the fields chosen for it, a null one inherits the account's.
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
                .await
                .map_err(crate::error::missing_ref_is_not_found)?;
        }
        None => iris_db::playback_preferences::set(state.db(), user.id, &prefs).await?,
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use iris_db::test_support::{make_user, migrated_pool};
    use iris_providers::ProviderRegistry;
    use tower::ServiceExt;

    use crate::state::AppState;

    #[tokio::test]
    async fn an_unknown_collection_is_a_404() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let state = AppState::for_tests(pool, ProviderRegistry::from_entries(&[]).unwrap()).await;
        let token = state.jwt().issue_access(user, false).unwrap();
        let req = Request::put("/api/me/playback-preferences")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(
                r#"{{"audio_language":"fr","collection_id":"{}"}}"#,
                uuid::Uuid::new_v4()
            )))
            .unwrap();
        let res = crate::app::build_router(state).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
