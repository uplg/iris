use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::routing::get;
use tower::Layer;
use tower_governor::GovernorLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::client_version::client_version_layer;
use crate::middleware::{coop_coep_layers, iris_caps_layer, static_cache_layer};
use crate::rate_limit::{self, Quota};
use crate::routes;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    // TWO rate-limit lanes on `/api/auth/*`, keyed per client IP
    // (`CF-Connecting-IP`; peer socket IP only for non-tunnelled/dev access —
    // see `rate_limit::CloudflareIpKeyExtractor`).
    //
    // NOTE on the key: every device in a household reaches Iris through the
    // same Cloudflare URL, so `CF-Connecting-IP` is the household's single
    // public (NAT) IP — ONE bucket shared by every browser, phone and TV in
    // the home. That makes the split below the real lever, not the key:
    //
    //  - STRICT (login / register): these run a ~100 ms Argon2 hash, so they
    //    are the brute-force / CPU surface. 5 req/s, burst 20 caps an
    //    attacker's verify spend while leaving real sign-ins untouched.
    //  - GENEROUS (refresh / logout / device pairing + polling): cheap,
    //    idempotent, token-protected, and hit on a routine cadence by every
    //    client at once (silent re-auth, keep-alive, TV poll every ~2 s). A 429
    //    here logs users out / breaks pairing, so the bucket is sized for the
    //    whole-household aggregate: 20 req/s, burst 60.
    //
    // Before the split, the TV's pairing polls shared the strict login bucket
    // with everyone's `/refresh`; draining it 429'd the TV (whose old client
    // then cleared the code and regenerated — a 429 feedback spiral) AND
    // collaterally 429'd browser refreshes into a logout. The generous lane
    // keeps the TV's poll answering cleanly so it never spirals.
    let login_governor = rate_limit::lane(Quota::LOGIN);
    let session_governor = rate_limit::lane(Quota::SESSION);
    rate_limit::spawn_sweeper(vec![login_governor.clone(), session_governor.clone()]);
    // Device-pairing endpoints are nested under the generous session lane
    // (rather than as a separate top-level group) because axum forbids
    // overlapping nest paths like `/auth` and `/auth/device`. Each subtree
    // keeps its own governor across the merge.
    let auth = routes::auth::strict_router()
        .merge(routes::passkeys::auth_router())
        .layer(GovernorLayer::new(login_governor.clone()))
        .merge(
            routes::auth::session_router()
                .nest("/device", routes::devices::auth_router())
                .layer(GovernorLayer::new(session_governor)),
        );
    // Same bucket as login: a stolen session guessing the current password
    // spends the household's sign-in budget, not one of its own.
    let me = routes::me::router()
        .merge(routes::me::password_router().layer(GovernorLayer::new(login_governor)))
        .nest("/devices", routes::devices::me_router())
        .nest("/passkeys", routes::passkeys::me_router())
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
        .nest("/livetv", routes::live_tv::router())
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
        if dist.is_dir() {
            tracing::info!(path = %dist.display(), "serving static frontend");
            app = app.fallback_service(static_router(&dist));
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

/// Asset directories of the web build: a missing file under one is a 404,
/// never the SPA's `index.html` (a 200 there reads as "present" to a
/// probe — the libav.js variant check — and as garbage to a loader).
const ASSET_DIRS: [&str; 6] = ["_app", "libavjs", "libass", "hevcjs", "libpgs", "fonts"];

/// The web build: asset directories as plain files, every other path the SPA
/// (its route or `index.html`), with the per-family `Cache-Control` of
/// `static_cache_layer` (here, not on the whole router, so `/api/*` keeps
/// its own).
fn static_router<S: Clone + Send + Sync + 'static>(dist: &std::path::Path) -> Router<S> {
    let mut static_app = Router::new();
    for dir in ASSET_DIRS {
        static_app = static_app.nest_service(&format!("/{dir}"), ServeDir::new(dist.join(dir)));
    }
    static_app
        .fallback_service(ServeDir::new(dist).fallback(ServeFile::new(dist.join("index.html"))))
        .layer(axum::middleware::from_fn(static_cache_layer))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use tower::ServiceExt;

    async fn get(app: &axum::Router, path: &str) -> (StatusCode, String) {
        let res = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let ty = res
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default();
        (res.status(), ty)
    }

    #[tokio::test]
    async fn a_missing_asset_is_a_404_not_the_spa() {
        let dist = std::env::temp_dir().join(format!("iris-static-{}", std::process::id()));
        std::fs::create_dir_all(dist.join("libavjs")).unwrap();
        std::fs::write(dist.join("index.html"), "<!doctype html>").unwrap();
        std::fs::write(dist.join("libavjs/libav.mjs"), "export {}").unwrap();
        let app: axum::Router = super::static_router(&dist);

        let (status, ty) = get(&app, "/libavjs/libav.mjs").await;
        assert_eq!(status, StatusCode::OK);
        assert!(ty.contains("javascript"), "{ty}");
        for missing in [
            "/libavjs/libav-6.10.9.0-iris.wasm.mjs",
            "/libass/x.wasm",
            "/hevcjs/x.js",
            "/libpgs/x.js",
            "/_app/immutable/x.js",
            "/fonts/x.woff2",
        ] {
            assert_eq!(
                get(&app, missing).await.0,
                StatusCode::NOT_FOUND,
                "{missing}"
            );
        }
        for route in ["/", "/watch/abc/0", "/admin"] {
            let (status, ty) = get(&app, route).await;
            assert_eq!(status, StatusCode::OK, "{route}");
            assert!(ty.starts_with("text/html"), "{route}: {ty}");
        }
        std::fs::remove_dir_all(&dist).unwrap();
    }
}
