//! Live TV endpoints: country picker, per-country channel lists, now/next
//! guide, and the signed HLS proxy every stream plays through.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{ApiError, ApiResult};
use crate::live_tv::{LiveTvError, LiveTvService, proxy};
use crate::routes::extract::AuthUser;
use crate::state::AppState;

use axum::routing::post;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/countries", get(live_countries))
        .route("/proxy", get(live_proxy))
        .route("/logo", get(live_logo))
        .route("/{country}/channels", get(live_channels))
        .route("/{country}/channels/{id}/master.m3u8", get(live_master))
        .route(
            "/{country}/channels/{id}/playback-error",
            post(live_playback_error),
        )
        .route("/{country}/epg/now", get(live_epg_now))
}

fn service(state: &AppState) -> Result<&LiveTvService, ApiError> {
    // Feature disabled in config → the whole subtree 404s.
    state.live_tv().ok_or(ApiError::NotFound)
}

impl From<LiveTvError> for ApiError {
    fn from(e: LiveTvError) -> Self {
        match e {
            LiveTvError::UnknownCountry | LiveTvError::UnknownChannel => ApiError::NotFound,
            LiveTvError::BadProxyRequest => ApiError::BadRequest("invalid proxy request".into()),
            LiveTvError::Upstream(msg) => ApiError::Upstream(msg),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LiveCountriesResponse {
    /// Country pre-selected by clients (config `live_tv.default_country`).
    pub default_country: String,
    pub countries: Vec<LiveCountry>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LiveCountry {
    /// ISO 3166-1 alpha-2, lowercase.
    pub code: String,
    pub name: String,
    /// Emoji flag.
    pub flag: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LiveChannelsResponse {
    pub country: String,
    pub channels: Vec<LiveChannel>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LiveChannel {
    /// Stable slug, unique within a country. Play via
    /// `/api/livetv/{country}/channels/{id}/master.m3u8`.
    pub id: String,
    pub name: String,
    pub logo_url: Option<String>,
    pub categories: Vec<String>,
    /// Best available vertical resolution when known (e.g. 1080).
    pub quality: Option<u32>,
    pub geo_blocked: bool,
    pub not_24_7: bool,
    /// French TNT number (Arcom) — set only for `fr` national networks;
    /// drives the pinned "TNT" section.
    pub tnt_number: Option<u16>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LiveEpgNowResponse {
    pub entries: Vec<LiveNowNext>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LiveNowNext {
    pub channel_id: String,
    pub now: Option<LiveProgramme>,
    pub next: Option<LiveProgramme>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LiveProgramme {
    pub title: String,
    pub start: DateTime<Utc>,
    pub stop: DateTime<Utc>,
    pub category: Option<String>,
    pub description: Option<String>,
}

impl From<crate::live_tv::epg::Programme> for LiveProgramme {
    fn from(p: crate::live_tv::epg::Programme) -> Self {
        Self {
            title: p.title,
            start: p.start,
            stop: p.stop,
            category: p.category,
            description: p.description,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct LiveProxyParams {
    /// Channel key (`country:id`) the URL was minted for.
    pub c: String,
    /// base64url-encoded upstream URL.
    pub u: String,
    /// HMAC signature over channel key + URL.
    pub s: String,
}

#[utoipa::path(
    get,
    path = "/api/livetv/countries",
    operation_id = "live_tv_countries",
    responses((status = 200, description = "Countries available in the live TV catalogue", body = LiveCountriesResponse)),
    tag = "live-tv",
)]
pub(crate) async fn live_countries(
    State(state): State<AppState>,
    _user: AuthUser,
) -> ApiResult<Json<LiveCountriesResponse>> {
    let svc = service(&state)?;
    let countries = svc.countries().await?;
    Ok(Json(LiveCountriesResponse {
        default_country: svc.default_country().to_string(),
        countries: countries
            .iter()
            .map(|c| LiveCountry {
                code: c.code.clone(),
                name: c.name.clone(),
                flag: c.flag.clone(),
            })
            .collect(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/livetv/{country}/channels",
    operation_id = "live_tv_channels",
    params(("country" = String, Path, description = "ISO 3166-1 alpha-2 country code")),
    responses(
        (status = 200, description = "Channels for the country — TNT-pinned first (fr), then grouped by category", body = LiveChannelsResponse),
        (status = 404, description = "Unknown country / live TV disabled"),
        (status = 502, description = "Upstream playlist unavailable"),
    ),
    tag = "live-tv",
)]
pub(crate) async fn live_channels(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(country): Path<String>,
) -> ApiResult<Json<LiveChannelsResponse>> {
    let svc = service(&state)?;
    let snap = svc.channels(&country).await?;
    Ok(Json(LiveChannelsResponse {
        country: country.to_lowercase(),
        channels: snap
            .channels
            .iter()
            .map(|c| LiveChannel {
                id: c.id.clone(),
                name: c.name.clone(),
                // Same-origin signed proxy URL: no hotlink CORS noise, and
                // clients can read pixels for the adaptive logo plate.
                logo_url: c.logo_url.as_deref().and_then(|u| svc.logo_proxy_url(u)),
                categories: c.categories.clone(),
                quality: c.sources.iter().filter_map(|s| s.quality).max(),
                geo_blocked: c.geo_blocked,
                not_24_7: c.not_24_7,
                tnt_number: c.tnt_number,
            })
            .collect(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/livetv/{country}/epg/now",
    operation_id = "live_tv_epg_now",
    params(("country" = String, Path, description = "ISO 3166-1 alpha-2 country code")),
    responses((status = 200, description = "Current + next programme per channel (channels without guide data are omitted; empty when the country has no guide source)", body = LiveEpgNowResponse)),
    tag = "live-tv",
)]
pub(crate) async fn live_epg_now(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(country): Path<String>,
) -> ApiResult<Json<LiveEpgNowResponse>> {
    let svc = service(&state)?;
    let entries = svc.epg_now(&country).await?;
    Ok(Json(LiveEpgNowResponse {
        entries: entries
            .into_iter()
            .map(|e| LiveNowNext {
                channel_id: e.channel_id,
                now: e.now.map(Into::into),
                next: e.next.map(Into::into),
            })
            .collect(),
        fetched_at: Utc::now(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/livetv/{country}/channels/{id}/master.m3u8",
    operation_id = "live_tv_master",
    params(
        ("country" = String, Path, description = "ISO 3166-1 alpha-2 country code"),
        ("id" = String, Path, description = "Channel id from the channel list"),
    ),
    responses(
        (status = 200, description = "HLS master playlist with every URI rewritten to the signed proxy", body = String, content_type = "application/vnd.apple.mpegurl"),
        (status = 404, description = "Unknown country / channel"),
        (status = 502, description = "Every upstream source for the channel is down"),
    ),
    tag = "live-tv",
)]
pub(crate) async fn live_master(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((country, id)): Path<(String, String)>,
) -> ApiResult<Response> {
    let svc = service(&state)?;
    let playlist = svc.master_playlist(&country, &id).await?;
    let mut resp = playlist_response(playlist.body)?;
    // Which upstream is actually being watched — one glance at the network
    // tab when someone reports bad sound/video on a channel.
    let headers = resp.headers_mut();
    if let Ok(v) = playlist.source_index.to_string().parse() {
        headers.insert("x-iris-live-source", v);
    }
    if let Ok(v) = playlist.upstream_host.parse() {
        headers.insert("x-iris-live-upstream", v);
    }
    Ok(resp)
}

#[utoipa::path(
    get,
    path = "/api/livetv/proxy",
    operation_id = "live_tv_proxy",
    params(LiveProxyParams),
    responses(
        (status = 200, description = "Proxied upstream bytes (media playlists re-rewritten, segments streamed through)", body = String, content_type = "application/octet-stream"),
        (status = 400, description = "Missing / invalid signature"),
        (status = 502, description = "Upstream fetch failed"),
    ),
    tag = "live-tv",
)]
pub(crate) async fn live_proxy(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(params): Query<LiveProxyParams>,
) -> ApiResult<Response> {
    let svc = service(&state)?;
    let (resp, final_url) = svc.proxy_fetch(&params.c, &params.u, &params.s).await?;
    let status = resp.status();
    let upstream_ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let is_playlist = proxy::is_playlist(&final_url, upstream_ct.as_deref());

    // Forward the upstream status verbatim (a live segment that has rolled
    // off the window legitimately 404s — the player retries / gap-skips it).
    // Only genuine connection failures became a 502 back in `proxy_fetch`.
    // For segments, feed the outcome into source health so a persistently
    // broken origin gets demoted and the next feed elected.
    if !is_playlist {
        svc.note_segment_result(&params.c, status.is_success())
            .await;
    }
    if !status.is_success() {
        return Response::builder()
            .status(status.as_u16())
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::empty())
            .map_err(|e| ApiError::Internal(e.into()));
    }

    // Nested playlists (media playlists reached through the master) get the
    // same rewrite treatment as the master itself.
    if is_playlist {
        let body = resp
            .text()
            .await
            .map_err(|e| ApiError::Upstream(e.to_string()))?;
        if body.len() > svc.max_playlist_bytes() {
            return Err(ApiError::Upstream("playlist too large".into()));
        }
        let rewritten = proxy::rewrite_playlist(&body, &final_url, &params.c, svc.signer());
        return playlist_response(rewritten);
    }

    // Segment / key passthrough: stream the body without buffering.
    let content_type = upstream_ct.unwrap_or_else(|| {
        match std::path::Path::new(final_url.path())
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("ts") => "video/mp2t",
            Some("m4s" | "mp4") => "video/mp4",
            Some("aac") => "audio/aac",
            _ => "application/octet-stream",
        }
        .to_string()
    });
    let content_length = resp.content_length();

    let mut builder = Response::builder()
        .status(resp.status().as_u16())
        .header(header::CONTENT_TYPE, content_type)
        // A cached live playlist or segment stalls playback instantly.
        .header(header::CACHE_CONTROL, "no-store");
    if let Some(len) = content_length {
        builder = builder.header(header::CONTENT_LENGTH, len);
    }
    builder
        .body(Body::from_stream(resp.bytes_stream()))
        .map_err(|e| ApiError::Internal(e.into()))
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct LiveLogoParams {
    /// base64url-encoded upstream logo URL.
    pub u: String,
    /// HMAC signature.
    pub s: String,
}

#[utoipa::path(
    get,
    path = "/api/livetv/logo",
    operation_id = "live_tv_logo",
    params(LiveLogoParams),
    responses(
        (status = 200, description = "Proxied channel logo bytes", body = String, content_type = "application/octet-stream"),
        (status = 400, description = "Missing / invalid signature"),
    ),
    tag = "live-tv",
)]
// No `AuthUser`: logos load from plain `<img>` / Coil without session
// plumbing; the HMAC signature (minted only by our channel-list response)
// is the access control, exactly like the stream proxy.
pub(crate) async fn live_logo(
    State(state): State<AppState>,
    Query(params): Query<LiveLogoParams>,
) -> ApiResult<Response> {
    let svc = service(&state)?;
    let logo = svc.fetch_logo(&params.u, &params.s).await?;
    if logo.status != 200 {
        // Dead / rate-limited logo host → forward a 404 so the card shows its
        // letter-tile fallback, and let the client cache that miss briefly so
        // it doesn't re-hammer us (and us the upstream) on every re-render.
        return Response::builder()
            .status(axum::http::StatusCode::NOT_FOUND)
            .header(header::CACHE_CONTROL, "public, max-age=300")
            .body(Body::empty())
            .map_err(|e| ApiError::Internal(e.into()));
    }
    Response::builder()
        .header(header::CONTENT_TYPE, logo.content_type)
        // Logos are effectively static — let clients cache for a day.
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(logo.bytes))
        .map_err(|e| ApiError::Internal(e.into()))
}

#[utoipa::path(
    post,
    path = "/api/livetv/{country}/channels/{id}/playback-error",
    operation_id = "live_tv_playback_error",
    params(
        ("country" = String, Path, description = "ISO 3166-1 alpha-2 country code"),
        ("id" = String, Path, description = "Channel id from the channel list"),
    ),
    responses(
        (status = 204, description = "Active source demoted; the next master request elects the next candidate"),
        (status = 404, description = "Unknown country / channel"),
    ),
    tag = "live-tv",
)]
pub(crate) async fn live_playback_error(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((country, id)): Path<(String, String)>,
) -> ApiResult<axum::http::StatusCode> {
    let svc = service(&state)?;
    svc.report_playback_failure(&country, &id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

fn playlist_response(body: String) -> ApiResult<Response> {
    Response::builder()
        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .map_err(|e| ApiError::Internal(e.into()))
}
