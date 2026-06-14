use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use iris_providers::registry::ProviderInfo;

use crate::error::ApiResult;
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list))
}

#[utoipa::path(
    get,
    path = "/api/providers",
    operation_id = "list_providers",
    responses((status = 200, description = "Registered search providers and their capabilities", body = [ProviderInfo])),
    tag = "providers",
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    _user: AuthUser,
) -> ApiResult<Json<Vec<ProviderInfo>>> {
    Ok(Json(state.providers().info()))
}
