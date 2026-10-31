//! Friends: asking, answering, and the activity friends see. What a profile
//! adds is visible to its friends; what it watches only once it turned
//! sharing on, and never a title it hid.

use std::collections::HashSet;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use iris_db::friends::{Event, Title};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;

/// Under `/me/friends`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(overview))
        .route("/search", get(search))
        .route("/activity", get(activity))
        .route("/sharing", put(set_sharing))
        .route("/hidden", post(hide).delete(unhide))
        .route("/{id}", delete(remove).post(ask))
        .route("/{id}/accept", post(accept))
        .route("/{id}/decline", post(decline))
}

const RECENT: i64 = 30;
const FOUND: i64 = 20;

#[derive(Debug, Serialize, ToSchema)]
pub struct PersonView {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HiddenTitleView {
    pub collection_id: Option<Uuid>,
    pub infohash: Option<String>,
    pub title: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FriendsOverview {
    pub friends: Vec<PersonView>,
    /// Requests waiting for this profile's answer.
    pub incoming: Vec<PersonView>,
    /// Requests this profile sent, still unanswered as far as it can tell.
    pub outgoing: Vec<PersonView>,
    /// Whether friends see what this profile watches.
    pub share_watching: bool,
    pub hidden: Vec<HiddenTitleView>,
}

#[utoipa::path(
    get,
    path = "/api/me/friends",
    operation_id = "friends_overview",
    responses((status = 200, description = "Friends, requests and what is shared", body = FriendsOverview)),
    tag = "friends",
)]
pub(crate) async fn overview(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<FriendsOverview>> {
    let mut view = FriendsOverview {
        friends: Vec::new(),
        incoming: Vec::new(),
        outgoing: Vec::new(),
        share_watching: iris_db::friends::sharing(state.db(), user.id).await?,
        hidden: iris_db::friends::hidden(state.db(), user.id)
            .await?
            .into_iter()
            .map(|h| HiddenTitleView {
                collection_id: h.collection_id,
                infohash: h.infohash,
                title: h.title,
            })
            .collect(),
    };
    for r in iris_db::friends::relations(state.db(), user.id).await? {
        let list = match (r.accepted, r.incoming) {
            (true, _) => &mut view.friends,
            (false, true) => &mut view.incoming,
            (false, false) => &mut view.outgoing,
        };
        list.push(PersonView {
            id: r.id,
            name: r.name,
        });
    }
    Ok(Json(view))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct SearchParams {
    /// Part of a display name, two characters at least.
    q: String,
}

#[utoipa::path(
    get,
    path = "/api/me/friends/search",
    operation_id = "friends_search",
    params(SearchParams),
    responses((status = 200, description = "Profiles to ask, by name; never an email", body = [PersonView])),
    tag = "friends",
)]
pub(crate) async fn search(
    State(state): State<AppState>,
    user: AuthUser,
    Query(params): Query<SearchParams>,
) -> ApiResult<Json<Vec<PersonView>>> {
    let q = params.q.trim();
    if q.chars().count() < 2 {
        return Ok(Json(Vec::new()));
    }
    let found = iris_db::friends::search(state.db(), user.id, q, FOUND).await?;
    Ok(Json(
        found
            .into_iter()
            .map(|p| PersonView {
                id: p.id,
                name: p.name,
            })
            .collect(),
    ))
}

#[utoipa::path(
    post,
    path = "/api/me/friends/{id}",
    operation_id = "friends_ask",
    params(("id" = Uuid, Path, description = "The profile to ask")),
    responses(
        (status = 204, description = "Asked; when they had asked first, you are friends"),
        (status = 404, description = "No such profile"),
    ),
    tag = "friends",
)]
pub(crate) async fn ask(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let other = UserId::from(id);
    if other == user.id {
        return Err(ApiError::BadRequest("that is you".into()));
    }
    if iris_db::users::find_by_id(state.db(), other)
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    iris_db::friends::request(state.db(), user.id, other).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn done(found: bool) -> ApiResult<StatusCode> {
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

#[utoipa::path(
    post,
    path = "/api/me/friends/{id}/accept",
    operation_id = "friends_accept",
    params(("id" = Uuid, Path, description = "Who asked")),
    responses(
        (status = 204, description = "You are friends"),
        (status = 404, description = "No such request"),
    ),
    tag = "friends",
)]
pub(crate) async fn accept(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    done(iris_db::friends::accept(state.db(), user.id, UserId::from(id)).await?)
}

#[utoipa::path(
    post,
    path = "/api/me/friends/{id}/decline",
    operation_id = "friends_decline",
    params(("id" = Uuid, Path, description = "Who asked")),
    responses(
        (status = 204, description = "Declined; they are not told"),
        (status = 404, description = "No such request"),
    ),
    tag = "friends",
)]
pub(crate) async fn decline(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    done(iris_db::friends::decline(state.db(), user.id, UserId::from(id)).await?)
}

#[utoipa::path(
    delete,
    path = "/api/me/friends/{id}",
    operation_id = "friends_remove",
    params(("id" = Uuid, Path, description = "The friend, or who was asked")),
    responses(
        (status = 204, description = "Friendship ended or request taken back"),
        (status = 404, description = "Nothing to remove"),
    ),
    tag = "friends",
)]
pub(crate) async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    done(iris_db::friends::remove(state.db(), user.id, UserId::from(id)).await?)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SharingRequest {
    pub share_watching: bool,
}

#[utoipa::path(
    put,
    path = "/api/me/friends/sharing",
    operation_id = "friends_set_sharing",
    request_body = SharingRequest,
    responses((status = 204, description = "Saved")),
    tag = "friends",
)]
pub(crate) async fn set_sharing(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<SharingRequest>,
) -> ApiResult<StatusCode> {
    iris_db::friends::set_sharing(state.db(), user.id, body.share_watching).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct HideRequest {
    pub infohash: String,
}

#[utoipa::path(
    post,
    path = "/api/me/friends/hidden",
    operation_id = "friends_hide",
    request_body = HideRequest,
    responses(
        (status = 204, description = "The release's whole title is kept from friends"),
        (status = 404, description = "No such release"),
    ),
    tag = "friends",
)]
pub(crate) async fn hide(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<HideRequest>,
) -> ApiResult<StatusCode> {
    done(iris_db::friends::hide(state.db(), user.id, &body.infohash).await?)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UnhideRequest {
    pub collection_id: Option<Uuid>,
    pub infohash: Option<String>,
}

#[utoipa::path(
    delete,
    path = "/api/me/friends/hidden",
    operation_id = "friends_unhide",
    request_body = UnhideRequest,
    responses((status = 204, description = "Friends can see it again")),
    tag = "friends",
)]
pub(crate) async fn unhide(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UnhideRequest>,
) -> ApiResult<StatusCode> {
    iris_db::friends::unhide(
        state.db(),
        user.id,
        body.collection_id,
        body.infohash.as_deref(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FriendAction {
    Watching,
    Watched,
    Added,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FriendEvent {
    pub friend_id: Uuid,
    pub friend_name: String,
    pub action: FriendAction,
    pub title: String,
    pub collection_id: Option<Uuid>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub at: DateTime<Utc>,
}

impl FriendEvent {
    fn of(
        friend_id: Uuid,
        friend_name: String,
        action: FriendAction,
        at: DateTime<Utc>,
        t: Title,
    ) -> Self {
        Self {
            friend_id,
            friend_name,
            action,
            title: t.title,
            collection_id: t.collection_id,
            season: t.season,
            episode: t.episode,
            at,
        }
    }

    /// One line per friend and title: an episode or release more of the same
    /// title says nothing new.
    fn key(&self) -> (Uuid, String) {
        let title = self
            .collection_id
            .map_or_else(|| self.title.clone(), |c| c.to_string());
        (self.friend_id, title)
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FriendActivity {
    /// What friends are playing this minute.
    pub live: Vec<FriendEvent>,
    /// What they watched and added lately, newest first.
    pub recent: Vec<FriendEvent>,
}

#[utoipa::path(
    get,
    path = "/api/me/friends/activity",
    operation_id = "friends_activity",
    responses((status = 200, description = "What friends watch and add", body = FriendActivity)),
    tag = "friends",
)]
pub(crate) async fn activity(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<FriendActivity>> {
    let sharing = iris_db::friends::sharing_friends(state.db(), user.id).await?;
    let sessions = state.presence().snapshot().await;
    let mut live = Vec::new();
    for friend in sharing {
        let Some(session) = sessions.iter().find(|s| s.user_id == friend.id) else {
            continue;
        };
        let shown = iris_db::friends::shown_title(
            state.db(),
            friend.id,
            &session.infohash,
            session.file_idx,
        )
        .await?;
        if let Some(title) = shown {
            live.push(FriendEvent::of(
                friend.id,
                friend.name,
                FriendAction::Watching,
                session.last_seen_at,
                title,
            ));
        }
    }

    let event =
        |action| move |e: Event| FriendEvent::of(e.friend_id, e.friend_name, action, e.at, e.title);
    let mut recent: Vec<_> = iris_db::friends::recently_watched(state.db(), user.id, RECENT)
        .await?
        .into_iter()
        .map(event(FriendAction::Watched))
        .chain(
            iris_db::friends::recently_added(state.db(), user.id, RECENT)
                .await?
                .into_iter()
                .map(event(FriendAction::Added)),
        )
        .collect();
    recent.sort_by_key(|e| std::cmp::Reverse(e.at));
    let mut seen: HashSet<_> = live.iter().map(FriendEvent::key).collect();
    recent.retain(|e| seen.insert(e.key()));
    recent.truncate(usize::try_from(RECENT).unwrap_or(usize::MAX));
    Ok(Json(FriendActivity { live, recent }))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::presence::{Heartbeat, PlaybackState};
    use crate::routes::auth::tests::{app_with_member, call};

    #[tokio::test]
    async fn live_watching_needs_a_friend_who_shares_and_never_shows_an_email() {
        let (state, app, me, _) = app_with_member().await;
        let friend = iris_db::test_support::make_named_user(state.db(), "Muriel").await;
        sqlx::query(
            "INSERT INTO torrents (id, infohash, name, total_size_bytes, added_by, added_at) \
             VALUES (?1, 'aaaa', 'Dune.2021', 1, ?2, ?3)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(uuid::Uuid::from(friend))
        .bind(chrono::Utc::now())
        .execute(state.db())
        .await
        .unwrap();
        state
            .presence()
            .touch(Heartbeat {
                user_id: friend.into(),
                infohash: "aaaa".into(),
                file_idx: 0,
                position_seconds: 1.0,
                duration_seconds: None,
                state: PlaybackState::Playing,
                client: None,
                client_version: None,
                browser: None,
            })
            .await;
        let mine = state.jwt().issue_access(me, false).unwrap();
        let theirs = state.jwt().issue_access(friend, false).unwrap();
        let read = async || {
            call(
                &app,
                "GET",
                "/api/me/friends/activity",
                Some(&mine),
                None,
                None,
            )
            .await
        };

        let found = call(
            &app,
            "GET",
            "/api/me/friends/search?q=mur",
            Some(&mine),
            None,
            None,
        )
        .await;
        assert_eq!(found.json[0]["name"], "Muriel");
        assert!(found.json[0].get("email").is_none());

        let path = format!("/api/me/friends/{}", uuid::Uuid::from(friend));
        assert_eq!(
            call(&app, "POST", &path, Some(&mine), None, None)
                .await
                .status,
            StatusCode::NO_CONTENT
        );
        assert_eq!(read().await.json["recent"], serde_json::json!([]));

        let accepted = format!("/api/me/friends/{}/accept", uuid::Uuid::from(me));
        assert_eq!(
            call(&app, "POST", &accepted, Some(&theirs), None, None)
                .await
                .status,
            StatusCode::NO_CONTENT
        );
        let friends = read().await;
        assert_eq!(friends.json["live"], serde_json::json!([]));
        assert_eq!(friends.json["recent"][0]["action"], "added");

        call(
            &app,
            "PUT",
            "/api/me/friends/sharing",
            Some(&theirs),
            None,
            Some(serde_json::json!({ "share_watching": true })),
        )
        .await;
        let live = read().await;
        assert_eq!(live.json["live"][0]["action"], "watching");
        assert_eq!(live.json["live"][0]["friend_name"], "Muriel");
    }

    #[tokio::test]
    async fn only_the_one_asked_can_answer() {
        let (state, app, me, _) = app_with_member().await;
        let other = iris_db::test_support::make_user(state.db()).await;
        let mine = state.jwt().issue_access(me, false).unwrap();
        iris_db::friends::request(state.db(), me, other)
            .await
            .unwrap();
        let path = format!("/api/me/friends/{}/accept", uuid::Uuid::from(other));
        assert_eq!(
            call(&app, "POST", &path, Some(&mine), None, None)
                .await
                .status,
            StatusCode::NOT_FOUND
        );
    }
}
