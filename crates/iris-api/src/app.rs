use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::routing::get;
use tower::Layer;
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::client_version::client_version_layer;
use crate::middleware::{coop_coep_layers, iris_caps_layer, static_cache_layer};
use crate::rate_limit::CloudflareIpKeyExtractor;
use crate::routes;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    // Per-CF-client-IP token bucket on `/api/auth/*` (login, register,
    // refresh, logout, device pairing). 5 req/s steady-state, burst
    // 20 — leaves normal multi-tab / multi-device usage unaffected
    // but caps an attacker's Argon2 spend at ~5 verifies/sec per
    // attacker IP. Key extraction reads `CF-Connecting-IP`; see
    // `rate_limit::CloudflareIpKeyExtractor` for the bypass story.
    let auth_governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(5)
            .burst_size(20)
            .key_extractor(CloudflareIpKeyExtractor)
            .finish()
            .expect("auth governor config is hard-coded and valid"),
    );
    // Device-pairing endpoints are merged INTO the auth and me routers
    // (rather than nested separately) because axum forbids overlapping
    // nest paths like `/auth` and `/auth/device`.
    let auth = routes::auth::router()
        .nest("/device", routes::devices::auth_router())
        .layer(GovernorLayer::new(auth_governor));
    let me = routes::me::router()
        .nest("/devices", routes::devices::me_router())
        .nest("/follows", routes::follows::router())
        .nest("/preferences", routes::preferences::router())
        .nest(
            "/playback-preferences",
            routes::playback_preferences::router(),
        )
        .nest("/for-you", routes::foryou::router())
        .nest("/moods", routes::moods::router());

    // Apply the Iris-Caps parser + telemetry on /torrents only — that's
    // where capability negotiation matters, and the middleware reads the DB
    // pool from state, which would be wasteful on /search etc.
    let torrents = routes::torrents::router().layer(axum::middleware::from_fn_with_state(
        state.clone(),
        iris_caps_layer,
    ));

    // Apply the X-Iris-Client version gate to every /api route EXCEPT
    // /health (kept reachable for monitoring / readiness probes) — when
    // a deployed APK is below the minimum, this returns 426 with a
    // structured body and the client surfaces "please update". Header
    // is always parsed + logged for telemetry, regardless of the gate.
    let gated = Router::new()
        .nest("/auth", auth)
        .nest("/admin", routes::admin::router())
        .nest("/me", me)
        .nest("/search", routes::search::router())
        .nest("/genres", routes::preferences::genres_router())
        .nest("/languages", routes::preferences::languages_router())
        .nest("/discover", routes::discover::router())
        .nest("/library", routes::library::router())
        .nest("/torrents", torrents)
        .nest("/providers", routes::providers::router())
        .nest("/metadata", routes::metadata::router())
        .layer(axum::middleware::from_fn(client_version_layer));
    let api = Router::new()
        .route("/health", get(routes::health::get))
        .merge(gated);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .max_age(Duration::from_mins(10));

    let mut app = Router::new().nest("/api", api);

    if let Some(dist) = state.cfg().server.web_dist.clone() {
        let index = dist.join("index.html");
        if dist.is_dir() {
            tracing::info!(path = %dist.display(), "serving static frontend");
            let serve = ServeDir::new(&dist).fallback(ServeFile::new(&index));
            // Differentiated Cache-Control per path family — see
            // `static_cache_layer` for the policy table. Applied here
            // (not on the whole router) so `/api/*` keeps its own
            // per-route cache headers untouched.
            let static_app = Router::new()
                .fallback_service(serve)
                .layer(axum::middleware::from_fn(static_cache_layer));
            app = app.fallback_service(static_app);
        } else {
            tracing::warn!(path = %dist.display(), "web_dist not found, skipping static serving");
        }
    }

    // COOP/COEP enable cross-origin isolation so the web client can use
    // SharedArrayBuffer for libav.js threads and SubtitlesOctopus. Applied
    // on every response (API + static); for /api the headers are harmless
    // extras.
    let (opener_policy, embedder_policy) = coop_coep_layers();
    app.layer(CompressionLayer::new())
        .layer(cors)
        .layer(opener_policy)
        .layer(embedder_policy)
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
