use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::routing::get;
use tower::Layer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::routes;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(routes::health::get))
        .nest("/auth", routes::auth::router())
        .nest("/admin", routes::admin::router())
        .nest("/me", routes::me::router())
        .nest("/search", routes::search::router())
        .nest("/torrents", routes::torrents::router())
        .nest("/providers", routes::providers::router())
        .nest("/metadata", routes::metadata::router());

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .max_age(Duration::from_secs(600));

    let mut app = Router::new().nest("/api", api);

    if let Some(dist) = state.cfg().server.web_dist.clone() {
        let index = dist.join("index.html");
        if dist.is_dir() {
            tracing::info!(path = %dist.display(), "serving static frontend");
            let serve = ServeDir::new(&dist).fallback(ServeFile::new(&index));
            app = app.fallback_service(serve);
        } else {
            tracing::warn!(path = %dist.display(), "web_dist not found, skipping static serving");
        }
    }

    app.layer(CompressionLayer::new())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Wrap the router so trailing slashes (`/api/search/`) are normalized away
/// before routing, which otherwise fall through to the SPA fallback.
pub fn into_service(
    router: Router,
) -> impl tower::Service<
    Request,
    Response = axum::response::Response,
    Error = std::convert::Infallible,
    Future = impl Send,
> + Clone
+ Send {
    NormalizePathLayer::trim_trailing_slash().layer(router)
}
