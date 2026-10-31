//! Profiles of one account ("Vincent", "Muriel"): each is a `users` row of its
//! own, so everything personal is already theirs. Changing profile is a new
//! session on the other row; the account's credentials stay on the account.

use axum::extract::{Path, State};
use axum::routing::{delete, get};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use iris_core::ids::UserId;
use iris_core::user::User;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::auth::UserResponse;
use crate::routes::extract::{AuthUser, REFRESH_COOKIE};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", delete(remove))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProfileView {
    pub id: Uuid,
    pub name: String,
    /// The account itself: it cannot be removed.
    pub main: bool,
    /// The profile this session is on.
    pub current: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateProfileRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SwitchProfileRequest {
    pub profile_id: Uuid,
}

async fn current(state: &AppState, user: &AuthUser) -> ApiResult<User> {
    iris_db::users::find_by_id(state.db(), user.id)
        .await?
        .ok_or(ApiError::Unauthorized)
}

/// For what only the account does (its password, its passkeys).
pub(crate) async fn the_account(state: &AppState, user: &AuthUser) -> ApiResult<()> {
    if current(state, user).await?.account_id.is_some() {
        return Err(ApiError::Refused {
            code: "account_only",
            message: "Only the main profile can do this.".into(),
        });
    }
    Ok(())
}

async fn views(state: &AppState, me: &User) -> ApiResult<Vec<ProfileView>> {
    let profiles = iris_db::users::profiles_of(state.db(), me.account()).await?;
    Ok(profiles
        .into_iter()
        .map(|p| ProfileView {
            id: p.id.into(),
            main: p.account_id.is_none(),
            current: p.id == me.id,
            name: p.display_name,
        })
        .collect())
}

#[utoipa::path(
    get,
    path = "/api/me/profiles",
    operation_id = "list_profiles",
    responses((status = 200, description = "The account's profiles, itself first", body = [ProfileView])),
    tag = "me",
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<ProfileView>>> {
    let me = current(&state, &user).await?;
    Ok(Json(views(&state, &me).await?))
}

#[utoipa::path(
    post,
    path = "/api/me/profiles",
    operation_id = "create_profile",
    request_body = CreateProfileRequest,
    responses(
        (status = 200, description = "The account's profiles, the new one included", body = [ProfileView]),
        (status = 400, description = "Empty or too long a name"),
        (status = 409, description = "The account holds as many profiles as it can"),
    ),
    tag = "me",
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateProfileRequest>,
) -> ApiResult<Json<Vec<ProfileView>>> {
    let name = crate::routes::me::checked_display_name(&body.name)?;
    let me = current(&state, &user).await?;
    if iris_db::users::create_profile(state.db(), me.account(), name)
        .await?
        .is_none()
    {
        return Err(ApiError::Refused {
            code: "profiles_full",
            message: format!(
                "An account holds {} profiles at most.",
                iris_db::users::MAX_PROFILES
            ),
        });
    }
    Ok(Json(views(&state, &me).await?))
}

#[utoipa::path(
    delete,
    path = "/api/me/profiles/{id}",
    operation_id = "remove_profile",
    params(("id" = Uuid, Path, description = "The profile to remove")),
    responses(
        (status = 204, description = "Profile removed; what it grabbed stays in the library"),
        (status = 404, description = "No such profile on this account"),
        (status = 409, description = "The main profile, or the one in use"),
    ),
    tag = "me",
)]
pub(crate) async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    let me = current(&state, &user).await?;
    let target = sibling(&state, &me, UserId::from(id)).await?;
    if target.account_id.is_none() || target.id == me.id {
        return Err(ApiError::Refused {
            code: "profile_kept",
            message: "The main profile and the one in use stay.".into(),
        });
    }
    iris_db::users::delete(state.db(), target.id, me.account()).await?;
    state.session_cuts().forget();
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// `id` when it is a profile of the caller's account.
async fn sibling(state: &AppState, me: &User, id: UserId) -> ApiResult<User> {
    iris_db::users::find_by_id(state.db(), id)
        .await?
        .filter(|u| u.account() == me.account())
        .ok_or(ApiError::NotFound)
}

/// Under `/api/auth`, the only path the refresh cookie is sent to: the session
/// being left is ended, and a paired device keeps its label on the new one.
#[utoipa::path(
    post,
    path = "/api/auth/switch-profile",
    request_body = SwitchProfileRequest,
    responses(
        (status = 200, description = "On the other profile; new session cookies set", body = UserResponse),
        (status = 404, description = "No such profile on this account"),
    ),
    tag = "auth",
)]
pub(crate) async fn switch(
    State(state): State<AppState>,
    jar: CookieJar,
    user: AuthUser,
    Json(body): Json<SwitchProfileRequest>,
) -> ApiResult<(CookieJar, Json<UserResponse>)> {
    let me = current(&state, &user).await?;
    let target = sibling(&state, &me, UserId::from(body.profile_id)).await?;

    let leaving = jar
        .get(REFRESH_COOKIE)
        .and_then(|c| state.jwt().verify_refresh(c.value()).ok())
        .map(|claims| claims.jti);
    let device = match leaving {
        Some(jti) => iris_db::refresh_tokens::device_of(state.db(), jti).await?,
        None => None,
    };
    let (label, kind) = device.unwrap_or_default();

    let new =
        crate::routes::auth::new_session(&state, target.id, label.as_deref(), kind.as_deref());
    iris_db::refresh_tokens::insert_session(state.db(), &new).await?;
    if let Some(jti) = leaving {
        iris_db::refresh_tokens::revoke_for_user(state.db(), me.id, jti).await?;
    }
    let jar = crate::routes::auth::session_cookies(
        &state,
        &jar,
        target.id,
        target.is_admin,
        &crate::routes::auth::stored(&new),
    )?;
    Ok((jar, Json(target.into())))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use serde_json::json;

    use crate::routes::auth::tests::{PASSWORD, app_with_member, call, login};

    #[tokio::test]
    async fn a_profile_is_its_own_person_on_the_same_account() {
        let (state, app, account, email) = app_with_member().await;
        let signed_in = login(&app, &email, PASSWORD).await;
        let access = signed_in.cookie("iris_access").unwrap();
        let refresh = signed_in.cookie("iris_refresh").unwrap();
        let both = format!("{access}; {refresh}");

        let made = call(
            &app,
            "POST",
            "/api/me/profiles",
            None,
            Some(&both),
            Some(json!({ "name": "Muriel" })),
        )
        .await;
        assert_eq!(made.status, StatusCode::OK);
        let muriel = made.json[1]["id"].as_str().unwrap().to_owned();
        assert_eq!(made.json[0]["main"], true);
        assert_eq!(made.json[1]["current"], false);

        let switched = call(
            &app,
            "POST",
            "/api/auth/switch-profile",
            None,
            Some(&both),
            Some(json!({ "profile_id": muriel })),
        )
        .await;
        assert_eq!(switched.status, StatusCode::OK);
        assert_eq!(switched.json["display_name"], "Muriel");
        assert_eq!(switched.json["is_admin"], false);
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/auth/refresh",
                None,
                Some(&refresh),
                None
            )
            .await
            .status,
            StatusCode::UNAUTHORIZED,
            "the session left is over"
        );

        let as_muriel = switched.cookie("iris_access").unwrap();
        let on_muriel = call(
            &app,
            "GET",
            "/api/me/profiles",
            None,
            Some(&as_muriel),
            None,
        )
        .await;
        assert_eq!(on_muriel.json[1]["current"], true);
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/me/password",
                None,
                Some(&as_muriel),
                Some(json!({ "old_password": "x", "new_password": "a brand new secret" })),
            )
            .await
            .status,
            StatusCode::CONFLICT,
            "the password is the account's"
        );

        let back = state.jwt().issue_access(account, false).unwrap();
        let path = format!("/api/me/profiles/{muriel}");
        assert_eq!(
            call(&app, "DELETE", &path, None, Some(&as_muriel), None)
                .await
                .status,
            StatusCode::CONFLICT,
            "not the one in use"
        );
        assert_eq!(
            call(&app, "DELETE", &path, Some(&back), None, None)
                .await
                .status,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            call(&app, "GET", "/api/me", None, Some(&as_muriel), None)
                .await
                .status,
            StatusCode::UNAUTHORIZED,
            "a removed profile's session is over"
        );
    }

    #[tokio::test]
    async fn a_stranger_is_not_a_profile() {
        let (state, app, _, email) = app_with_member().await;
        let other = iris_db::test_support::make_user(state.db()).await;
        let access = login(&app, &email, PASSWORD)
            .await
            .cookie("iris_access")
            .unwrap();
        let id = uuid::Uuid::from(other);
        let tried = call(
            &app,
            "POST",
            "/api/auth/switch-profile",
            None,
            Some(&access),
            Some(json!({ "profile_id": id })),
        )
        .await;
        assert_eq!(tried.status, StatusCode::NOT_FOUND);
        let path = format!("/api/me/profiles/{id}");
        assert_eq!(
            call(&app, "DELETE", &path, None, Some(&access), None)
                .await
                .status,
            StatusCode::NOT_FOUND
        );
    }
}
