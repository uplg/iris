use axum::Json;
use serde_json::{Value, json};

#[utoipa::path(
    get,
    path = "/api/health",
    operation_id = "healthcheck",
    responses((status = 200, description = "Liveness probe — `{ \"status\": \"ok\" }` whenever reachable")),
    tag = "health",
)]
pub async fn get() -> Json<Value> {
    Json(json!({"status": "ok"}))
}
