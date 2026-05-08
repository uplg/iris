use axum::Json;
use serde_json::{Value, json};

pub async fn get() -> Json<Value> {
    Json(json!({"status": "ok"}))
}
