use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use iris_providers::ProviderRegistry;
use iris_providers::registry::{ProviderInfo, ProviderStatus};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::{AdminUser, AuthUser, Path};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list))
}

/// `/api/admin/providers…`, nested by the admin router.
pub fn admin_router() -> Router<AppState> {
    Router::new()
        .route("/providers", get(admin_list))
        .route("/providers/{id}", axum::routing::put(admin_set))
}

#[utoipa::path(
    get,
    path = "/api/providers",
    operation_id = "list_providers",
    responses((status = 200, description = "Enabled search providers and their capabilities", body = [ProviderInfo])),
    tag = "providers",
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    _user: AuthUser,
) -> ApiResult<Json<Vec<ProviderInfo>>> {
    Ok(Json(state.providers().info()))
}

/// Apply the admins' persisted on/off switches to a freshly built registry.
/// A row for a tracker the config no longer builds is kept, inert.
pub(crate) async fn load_overrides(pool: &iris_db::SqlitePool, registry: &ProviderRegistry) {
    match iris_db::provider_overrides::list(pool).await {
        Ok(rows) => {
            for (id, enabled) in rows {
                if registry.set_enabled(&id, enabled) && !enabled {
                    tracing::info!(provider = %id, "provider turned off in Admin; skipped");
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "loading provider overrides failed; all on"),
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/providers",
    operation_id = "admin_list_providers",
    responses(
        (status = 200, description = "Every providers.toml entry with its runtime state", body = [ProviderStatus]),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn admin_list(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<Vec<ProviderStatus>>> {
    Ok(Json(state.providers().statuses()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct SetProviderEnabled {
    enabled: bool,
}

/// Turn a tracker on or off without a restart. Off, it is asked for nothing
/// (searches, feeds, sweeps, grabs); what it already delivered keeps seeding
/// and its cached offers come back when it is on again.
#[utoipa::path(
    put,
    path = "/api/admin/providers/{id}",
    operation_id = "admin_set_provider_enabled",
    params(("id" = String, Path)),
    request_body = SetProviderEnabled,
    responses(
        (status = 200, description = "The provider's new state", body = ProviderStatus),
        (status = 403, description = "Caller is not an admin"),
        (status = 404, description = "No such provider in providers.toml"),
        (status = 409, description = "Disabled in providers.toml: only the config can turn it on"),
    ),
    tag = "admin",
)]
pub(crate) async fn admin_set(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<SetProviderEnabled>,
) -> ApiResult<Json<ProviderStatus>> {
    let registry = state.providers();
    let find = || registry.statuses().into_iter().find(|s| s.id == id);
    let current = find().ok_or(ApiError::NotFound)?;
    if !current.configured {
        return Err(ApiError::Conflict(format!(
            "{id} is disabled in providers.toml; only the config can turn it on"
        )));
    }
    iris_db::provider_overrides::set(state.db(), &id, body.enabled, admin.0.id).await?;
    registry.set_enabled(&id, body.enabled);
    super::audit(
        &state,
        admin.0.id,
        if body.enabled {
            "provider.enable"
        } else {
            "provider.disable"
        },
        "provider",
        Some(&id),
        Some(&id),
    )
    .await;
    find().map(Json).ok_or(ApiError::NotFound)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use iris_config::ProviderEntry;
    use iris_db::test_support::{make_user, migrated_pool};
    use iris_providers::ProviderRegistry;
    use tower::ServiceExt;

    use crate::state::AppState;

    fn registry() -> ProviderRegistry {
        let dead = "http://127.0.0.1:1";
        let entries: Vec<ProviderEntry> = [
            serde_json::json!({ "id": "nyaa", "kind": "nyaa", "base_url": dead }),
            serde_json::json!({ "id": "tr4ker", "kind": "nyaa", "base_url": dead }),
            serde_json::json!({ "id": "tos", "kind": "unit3d", "enabled": false }),
        ]
        .into_iter()
        .map(|v| serde_json::from_value(v).expect("provider entry"))
        .collect();
        ProviderRegistry::from_entries(&entries).expect("registry")
    }

    async fn call(
        app: &axum::Router,
        method: &str,
        path: &str,
        token: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_owned())))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn an_admin_switches_a_tracker_off_and_it_stays_off_across_a_restart() {
        let pool = migrated_pool().await;
        let admin = make_user(&pool).await;
        let member = make_user(&pool).await;
        let state = AppState::for_tests(pool.clone(), registry()).await;
        let admin_token = state.jwt().issue_access(admin, true).unwrap();
        let member_token = state.jwt().issue_access(member, false).unwrap();
        let app = crate::app::build_router(state.clone());

        let (status, _) = call(&app, "GET", "/api/admin/providers", &member_token, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let off = Some(r#"{"enabled":false}"#);
        let (status, _) = call(
            &app,
            "PUT",
            "/api/admin/providers/tr4ker",
            &member_token,
            off,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(state.providers().is_enabled("tr4ker"));

        let (status, list) = call(&app, "GET", "/api/admin/providers", &admin_token, None).await;
        assert_eq!(status, StatusCode::OK);
        let ids: Vec<&str> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["nyaa", "tos", "tr4ker"]);
        assert_eq!(list[1]["configured"], false);

        let (status, row) = call(
            &app,
            "PUT",
            "/api/admin/providers/tr4ker",
            &admin_token,
            off,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(row["enabled"], false);
        assert!(!state.providers().is_enabled("tr4ker"));
        assert_eq!(state.providers().ids(), ["nyaa"]);

        let on = Some(r#"{"enabled":true}"#);
        let (status, _) = call(&app, "PUT", "/api/admin/providers/tos", &admin_token, on).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = call(&app, "PUT", "/api/admin/providers/nope", &admin_token, on).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let audit = iris_db::audit::list(&pool, 10, 0).await.unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].action, "provider.disable");
        assert_eq!(audit[0].resource_id.as_deref(), Some("tr4ker"));

        let rebooted = registry();
        assert!(rebooted.is_enabled("tr4ker"));
        super::load_overrides(&pool, &rebooted).await;
        assert!(!rebooted.is_enabled("tr4ker") && rebooted.is_enabled("nyaa"));
    }

    #[tokio::test]
    async fn a_grab_from_a_tracker_turned_off_is_refused_with_its_own_code() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let state = AppState::for_tests(pool, registry()).await;
        state.providers().set_enabled("tr4ker", false);
        let token = state.jwt().issue_access(user, false).unwrap();
        let app = crate::app::build_router(state.clone());

        let grab = Some(r#"{"provider_id":"tr4ker","external_id":"42"}"#);
        for path in ["/api/torrents", "/api/torrents/preview"] {
            let (status, body) = call(&app, "POST", path, &token, grab).await;
            assert_eq!(status, StatusCode::CONFLICT, "{path}");
            assert_eq!(body["error"], "provider_off", "{path}");
            assert_eq!(body["message"], "This tracker is turned off in Admin.");
        }
        let (status, body) = call(
            &app,
            "GET",
            "/api/search/details?provider=tr4ker&id=42",
            &token,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "provider_off");
    }
}
