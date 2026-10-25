//! Admin: share a downloaded film with `SéléoTV` for a few days (see
//! [`crate::seleo`]). One file per share, the film's main video, of a finished
//! movie torrent; its metadata is read from TMDB once, at share time.

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::{AdminUser, Infohash, Path};
use crate::state::AppState;

const DEFAULT_DAYS: u32 = 7;
const MAX_DAYS: u32 = 60;

pub fn admin_router() -> Router<AppState> {
    Router::new().route("/seleo/shares", get(list)).route(
        "/seleo/shares/{infohash}/{file_idx}",
        axum::routing::put(share).delete(unshare),
    )
}

/// A film on `SéléoTV`, as the admin sees it.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SeleoShareView {
    infohash: String,
    file_idx: i64,
    /// The film's library title page.
    collection_id: Option<Uuid>,
    /// TMDB's French title, else the library's.
    title: String,
    year: Option<i64>,
    tmdb_id: Option<i64>,
    /// The file, relative to the download dir.
    path: String,
    size_bytes: i64,
    has_poster: bool,
    /// `None` once that admin's account is deleted.
    shared_by_name: Option<String>,
    shared_at: DateTime<Utc>,
    until: DateTime<Utc>,
}

impl From<iris_db::seleo_shares::ShareRow> for SeleoShareView {
    fn from(s: iris_db::seleo_shares::ShareRow) -> Self {
        Self {
            infohash: s.infohash,
            file_idx: s.file_idx,
            collection_id: s.collection_id,
            title: s.title,
            year: s.year,
            tmdb_id: s.tmdb_id,
            path: s.path,
            size_bytes: s.size_bytes,
            has_poster: s.poster.is_some(),
            shared_by_name: s.shared_by_name,
            shared_at: s.shared_at,
            until: s.until,
        }
    }
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub(crate) struct ShareRequest {
    /// How long: from now for a new share, past its end for a running one.
    /// 7 when absent, clamped to 1..=60.
    #[serde(default)]
    days: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/api/admin/seleo/shares",
    operation_id = "admin_list_seleo_shares",
    responses(
        (status = 200, description = "The films on SéléoTV now, newest share first", body = [SeleoShareView]),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<Vec<SeleoShareView>>> {
    let rows = iris_db::seleo_shares::list_live(state.db(), Utc::now()).await?;
    Ok(Json(rows.into_iter().map(SeleoShareView::from).collect()))
}

/// Share the film's file for `days`. A running share is extended by as much
/// (never past 60 days from now) and keeps its metadata; a new one reads
/// TMDB and the poster.
#[utoipa::path(
    put,
    path = "/api/admin/seleo/shares/{infohash}/{file_idx}",
    operation_id = "admin_share_with_seleo",
    params(("infohash" = String, Path), ("file_idx" = u32, Path)),
    request_body = ShareRequest,
    responses(
        (status = 200, description = "The share, new or extended", body = SeleoShareView),
        (status = 400, description = "`not_the_film`: not the film's main video file"),
        (status = 403, description = "Caller is not an admin"),
        (status = 404, description = "No such torrent on disk"),
        (status = 409, description = "`not_a_movie`, `not_downloaded`, `not_in_engine` or `file_missing`"),
    ),
    tag = "admin",
)]
pub(crate) async fn share(
    State(state): State<AppState>,
    admin: AdminUser,
    Path((infohash, file_idx)): Path<(Infohash, usize)>,
    Json(body): Json<ShareRequest>,
) -> ApiResult<Json<SeleoShareView>> {
    let idx = i64::try_from(file_idx).map_err(|_| ApiError::NotFound)?;
    let days = Duration::days(i64::from(
        body.days.unwrap_or(DEFAULT_DAYS).clamp(1, MAX_DAYS),
    ));
    let now = Utc::now();
    let db = state.db();
    let dir = crate::seleo::share_dir(state.cfg());
    let running = iris_db::seleo_shares::get(db, &infohash, idx)
        .await?
        .filter(|s| s.until > now);
    let extended = match running {
        Some(s) => {
            let until = (s.until + days).min(now + Duration::days(i64::from(MAX_DAYS)));
            iris_db::seleo_shares::extend(db, &infohash, idx, until).await?
        }
        None => false,
    };
    if !extended {
        let until = now + days;
        let film = shareable(&state, &infohash, file_idx).await?;
        let meta = snapshot(&state, &film, &dir).await;
        // Under the infohash lock, as a delete is: the torrent is either
        // still on disk now, or its delete drops this share once done.
        let _held = state.engine().lock(&infohash).await;
        if iris_db::torrents::find_by_infohash(db, &infohash)
            .await?
            .is_none_or(|r| r.deleted_at.is_some())
        {
            return Err(ApiError::NotFound);
        }
        iris_db::seleo_shares::put(
            db,
            &iris_db::seleo_shares::NewShare {
                infohash: &infohash,
                file_idx: idx,
                shared_by: admin.0.id,
                until,
                path: &film.path,
                size_bytes: film.size_bytes,
                tmdb_id: meta.tmdb_id,
                title: &meta.title,
                year: meta.year,
                overview: meta.overview.as_deref(),
                runtime_minutes: meta.runtime_minutes,
                poster: meta.poster.as_deref(),
            },
        )
        .await?;
    }
    crate::seleo::publish(db, &dir).await;
    let row = iris_db::seleo_shares::get(db, &infohash, idx)
        .await?
        .ok_or(ApiError::NotFound)?;
    super::audit(
        &state,
        admin.0.id,
        if extended {
            "seleo.extend"
        } else {
            "seleo.share"
        },
        "torrent",
        Some(&infohash),
        Some(&row.title),
    )
    .await;
    Ok(Json(row.into()))
}

#[utoipa::path(
    delete,
    path = "/api/admin/seleo/shares/{infohash}/{file_idx}",
    operation_id = "admin_unshare_from_seleo",
    params(("infohash" = String, Path), ("file_idx" = u32, Path)),
    responses(
        (status = 204, description = "No longer on SéléoTV"),
        (status = 403, description = "Caller is not an admin"),
        (status = 404, description = "No such share"),
    ),
    tag = "admin",
)]
pub(crate) async fn unshare(
    State(state): State<AppState>,
    admin: AdminUser,
    Path((infohash, file_idx)): Path<(Infohash, usize)>,
) -> ApiResult<StatusCode> {
    let idx = i64::try_from(file_idx).map_err(|_| ApiError::NotFound)?;
    let dir = crate::seleo::share_dir(state.cfg());
    let title = iris_db::seleo_shares::get(state.db(), &infohash, idx)
        .await?
        .map(|s| s.title);
    if !crate::seleo::unshare(state.db(), &dir, &infohash, idx).await? {
        return Err(ApiError::NotFound);
    }
    super::audit(
        &state,
        admin.0.id,
        "seleo.unshare",
        "torrent",
        Some(&infohash),
        title.as_deref(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// What a share points at, once the release may be shared.
struct Shareable {
    row: iris_db::torrents::TorrentRow,
    file_idx: i64,
    display_title: String,
    path: String,
    size_bytes: i64,
}

async fn shareable(state: &AppState, infohash: &str, file_idx: usize) -> ApiResult<Shareable> {
    let refused = |code: &'static str, message: &str| ApiError::Refused {
        code,
        message: message.to_owned(),
    };
    let row = iris_db::torrents::find_by_infohash(state.db(), infohash)
        .await?
        .filter(|r| r.deleted_at.is_none())
        .ok_or(ApiError::NotFound)?;
    let collection = match row.collection_id {
        Some(id) => iris_db::collections::get(state.db(), id).await?,
        None => None,
    };
    let Some(collection) = collection.filter(|c| !c.is_tv()) else {
        return Err(refused(
            "not_a_movie",
            "Only a film can be shared with SéléoTV.",
        ));
    };
    if row.finished_at.is_none() {
        return Err(refused(
            "not_downloaded",
            "Still downloading: a film is shared once it is fully on disk.",
        ));
    }
    let Some(snap) = state.engine().get_by_infohash(infohash) else {
        return Err(refused(
            "not_in_engine",
            "This release isn't loaded by the torrent engine right now.",
        ));
    };
    if iris_torrent::main_video_index(&snap.files) != Some(file_idx) {
        return Err(ApiError::Invalid {
            code: "not_the_film",
            message: "Only the film's main video file can be shared.".into(),
        });
    }
    let size = snap
        .files
        .iter()
        .find(|f| f.index == file_idx)
        .map_or(0, |f| f.size_bytes);
    let abs = state
        .engine()
        .file_path(infohash, file_idx)
        .map_err(super::torrents::map_engine_err)?;
    if !tokio::fs::try_exists(&abs).await.unwrap_or(false) {
        return Err(refused(
            "file_missing",
            "The film's file is missing from disk.",
        ));
    }
    let path = abs
        .strip_prefix(state.engine().download_dir())
        .ok()
        .and_then(|p| p.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "{} is not a UTF-8 path under the download dir",
                abs.display()
            ))
        })?;
    Ok(Shareable {
        row,
        file_idx: i64::try_from(file_idx).map_err(|_| ApiError::NotFound)?,
        display_title: collection.display_title,
        path,
        size_bytes: i64::try_from(size).unwrap_or(i64::MAX),
    })
}

/// The metadata a share keeps.
struct Snapshot {
    tmdb_id: Option<i64>,
    title: String,
    year: Option<i64>,
    overview: Option<String>,
    runtime_minutes: Option<i64>,
    poster: Option<String>,
}

/// TMDB's French metadata and poster when the film's match is trusted and
/// TMDB answers; else the library's title and the release's year, no poster.
async fn snapshot(state: &AppState, film: &Shareable, dir: &std::path::Path) -> Snapshot {
    let row = &film.row;
    let tmdb_id = row.effective_tmdb_id().filter(|_| row.tmdb_verified);
    let release_year = iris_media::filename::parse(&row.name)
        .and_then(|p| p.year)
        .map(i64::from);
    let fetched = match (state.tmdb(), tmdb_id.and_then(|id| u64::try_from(id).ok())) {
        (Some(tmdb), Some(id)) => tmdb.movie_fr(id).await.map(|m| (tmdb, m)),
        _ => None,
    };
    let Some((tmdb, meta)) = fetched else {
        return Snapshot {
            tmdb_id,
            title: film.display_title.clone(),
            year: release_year,
            overview: None,
            runtime_minutes: None,
            poster: None,
        };
    };
    let id = crate::seleo::film_id(&row.infohash, film.file_idx);
    let poster = match meta.poster_path.as_deref() {
        Some(path) => crate::seleo::fetch_poster(tmdb, path, dir, &id).await,
        None => None,
    };
    Snapshot {
        tmdb_id,
        title: Some(meta.title)
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| film.display_title.clone()),
        year: meta.year.map(i64::from).or(release_year),
        overview: meta.overview,
        runtime_minutes: meta.runtime_minutes.map(i64::from),
        poster,
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use iris_db::collections::Kind;
    use iris_db::test_support::{make_user, migrated_pool};
    use iris_providers::ProviderRegistry;
    use serde_json::json;

    use crate::routes::auth::tests::call;
    use crate::state::AppState;

    const NAME: &str = "1917.2019.1080p.BluRay-GRP";

    /// A finished movie on disk (its film and a sample) and an unfinished
    /// series, each in the engine with its library row and collection.
    async fn library() -> (AppState, axum::Router, String, String, String, String) {
        let pool = migrated_pool().await;
        let admin = make_user(&pool).await;
        let member = make_user(&pool).await;
        let state =
            AppState::for_tests(pool.clone(), ProviderRegistry::from_entries(&[]).unwrap()).await;
        let dl = state.cfg().storage.download_dir.clone();
        let add = |name: &str, files: &[(&str, u64)], kind: Kind, finished: bool| {
            let (state, pool, dl) = (state.clone(), pool.clone(), dl.clone());
            let name = name.to_owned();
            let files: Vec<(String, u64)> =
                files.iter().map(|(p, l)| ((*p).to_owned(), *l)).collect();
            async move {
                for (p, len) in &files {
                    let path = dl.join(&name).join(p);
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, vec![7u8; usize::try_from(*len).unwrap()]).unwrap();
                }
                let refs: Vec<(&str, u64)> = files.iter().map(|(p, l)| (p.as_str(), *l)).collect();
                let bytes = iris_torrent::test_support::torrent_bytes(&name, &refs, "t");
                let added = state.engine().add_from_bytes(bytes).await.unwrap();
                let infohash = added.snapshot.infohash.clone();
                iris_db::torrents::upsert(
                    &pool,
                    iris_db::torrents::NewTorrent {
                        infohash: infohash.clone(),
                        name: name.clone(),
                        total_size_bytes: added.snapshot.total_size_bytes,
                        source_provider: None,
                        source_external_id: None,
                        tracker_tmdb_id: None,
                        added_by: member,
                    },
                )
                .await
                .unwrap();
                let c = iris_db::collections::create_standalone(&pool, "1917", kind)
                    .await
                    .unwrap();
                iris_db::torrents::set_collection(&pool, &infohash, Some(c.id))
                    .await
                    .unwrap();
                if finished {
                    iris_db::torrents::mark_finished(&pool, &infohash)
                        .await
                        .unwrap();
                }
                infohash
            }
        };
        let film = add(
            NAME,
            &[("1917.mkv", 40_000), ("Sample/sample.mkv", 1_000)],
            Kind::Movie,
            true,
        )
        .await;
        let show = add(
            "Show.S01.1080p-GRP",
            &[("e1.mkv", 20_000), ("e2.mkv", 20_000)],
            Kind::Tv,
            true,
        )
        .await;
        let admin_token = state.jwt().issue_access(admin, true).unwrap();
        let member_token = state.jwt().issue_access(member, false).unwrap();
        let app = crate::app::build_router(state.clone());
        (state, app, admin_token, member_token, film, show)
    }

    fn manifest(state: &AppState) -> serde_json::Value {
        let path = crate::seleo::share_dir(state.cfg()).join(crate::seleo::MANIFEST);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn only_an_admin_lists_shares_or_stops_them() {
        let (state, app, _, member, film, _) = library().await;
        let share = format!("/api/admin/seleo/shares/{film}/0");
        let body = Some(json!({ "days": 7 }));
        let list = call(
            &app,
            "GET",
            "/api/admin/seleo/shares",
            Some(&member),
            None,
            None,
        )
        .await;
        assert_eq!(list.status, StatusCode::FORBIDDEN);
        let put = call(&app, "PUT", &share, Some(&member), None, body).await;
        assert_eq!(put.status, StatusCode::FORBIDDEN);
        let del = call(&app, "DELETE", &share, Some(&member), None, None).await;
        assert_eq!(del.status, StatusCode::FORBIDDEN);
        let anon = call(&app, "GET", "/api/admin/seleo/shares", None, None, None).await;
        assert_eq!(anon.status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            iris_db::seleo_shares::list_live(state.db(), chrono::Utc::now())
                .await
                .unwrap(),
            Vec::new()
        );
    }

    #[tokio::test]
    async fn only_the_main_video_of_a_film_is_shared() {
        let (_, app, admin, _, film, show) = library().await;
        let sample = call(
            &app,
            "PUT",
            &format!("/api/admin/seleo/shares/{film}/1"),
            Some(&admin),
            None,
            Some(json!({})),
        )
        .await;
        assert_eq!(sample.status, StatusCode::BAD_REQUEST);
        assert_eq!(sample.json["error"], "not_the_film");
        let series = call(
            &app,
            "PUT",
            &format!("/api/admin/seleo/shares/{show}/0"),
            Some(&admin),
            None,
            Some(json!({})),
        )
        .await;
        assert_eq!(series.status, StatusCode::CONFLICT);
        assert_eq!(series.json["error"], "not_a_movie");
    }

    #[tokio::test]
    async fn an_admin_shares_the_film_which_then_cannot_be_deleted_until_unshared() {
        let (state, app, admin, member, film, _) = library().await;
        let share = format!("/api/admin/seleo/shares/{film}/0");
        let made = call(
            &app,
            "PUT",
            &share,
            Some(&admin),
            None,
            Some(json!({ "days": 365 })),
        )
        .await;
        assert_eq!(made.status, StatusCode::OK, "{}", made.json);
        assert_eq!(made.json["path"], format!("{NAME}/1917.mkv"));
        assert_eq!(made.json["size_bytes"], 40_000);
        assert_eq!(made.json["title"], "1917", "TMDB off: the library's title");
        assert_eq!(made.json["year"], 2019, "the release's year");
        assert_eq!(made.json["has_poster"], false);
        let until =
            chrono::DateTime::parse_from_rfc3339(made.json["until"].as_str().unwrap()).unwrap();
        let days = (until.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_hours();
        assert!(
            (59 * 24..=60 * 24).contains(&days),
            "clamped to 60 days: {days} h"
        );

        let films = manifest(&state)["films"].as_array().unwrap().clone();
        assert_eq!(films.len(), 1);
        assert_eq!(films[0]["id"], format!("{film}-0"));
        assert_eq!(films[0]["path"], format!("{NAME}/1917.mkv"));

        let extended = call(&app, "PUT", &share, Some(&admin), None, Some(json!({}))).await;
        assert_eq!(extended.status, StatusCode::OK);
        assert_eq!(extended.json["shared_at"], made.json["shared_at"]);
        let at = |v: &serde_json::Value| {
            chrono::DateTime::parse_from_rfc3339(v.as_str().unwrap()).unwrap()
        };
        let moved = at(&extended.json["until"]) - at(&made.json["until"]);
        assert!(
            moved < chrono::Duration::seconds(1),
            "never past 60 days: {moved}"
        );

        let refused = call(
            &app,
            "DELETE",
            &format!("/api/torrents/{film}"),
            Some(&member),
            None,
            None,
        )
        .await;
        assert_eq!(refused.status, StatusCode::CONFLICT);
        assert_eq!(refused.json["error"], "shared_with_seleo");
        assert!(state.engine().contains(&film));

        let list = call(
            &app,
            "GET",
            "/api/admin/seleo/shares",
            Some(&admin),
            None,
            None,
        )
        .await;
        assert_eq!(list.json.as_array().unwrap().len(), 1);
        let stopped = call(&app, "DELETE", &share, Some(&admin), None, None).await;
        assert_eq!(stopped.status, StatusCode::NO_CONTENT);
        let again = call(&app, "DELETE", &share, Some(&admin), None, None).await;
        assert_eq!(again.status, StatusCode::NOT_FOUND);
        assert_eq!(manifest(&state)["films"], json!([]));

        let deleted = call(
            &app,
            "DELETE",
            &format!("/api/torrents/{film}"),
            Some(&member),
            None,
            None,
        )
        .await;
        assert_eq!(deleted.status, StatusCode::NO_CONTENT);
        let gone = call(&app, "PUT", &share, Some(&admin), None, Some(json!({}))).await;
        assert_eq!(gone.status, StatusCode::NOT_FOUND);

        let audit: Vec<String> = iris_db::audit::list(state.db(), 10, 0)
            .await
            .unwrap()
            .into_iter()
            .map(|a| a.action)
            .collect();
        for action in ["seleo.share", "seleo.extend", "seleo.unshare"] {
            assert!(audit.iter().any(|a| a == action), "{action} in {audit:?}");
        }
    }

    #[tokio::test]
    async fn a_film_still_downloading_is_not_shared() {
        let (state, app, admin, _, film, _) = library().await;
        sqlx::query("UPDATE torrents SET finished_at = NULL WHERE infohash = ?1")
            .bind(&film)
            .execute(state.db())
            .await
            .unwrap();
        let r = call(
            &app,
            "PUT",
            &format!("/api/admin/seleo/shares/{film}/0"),
            Some(&admin),
            None,
            Some(json!({})),
        )
        .await;
        assert_eq!(r.status, StatusCode::CONFLICT);
        assert_eq!(r.json["error"], "not_downloaded");
    }
}
