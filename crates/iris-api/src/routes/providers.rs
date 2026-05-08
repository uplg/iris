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

async fn list(
    State(state): State<AppState>,
    _user: AuthUser,
) -> ApiResult<Json<Vec<ProviderInfo>>> {
    Ok(Json(state.providers().info()))
}
