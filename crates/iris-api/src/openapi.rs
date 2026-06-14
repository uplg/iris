//! `OpenAPI` spec assembly.
//!
//! utoipa derives the document from the annotated handlers
//! (`#[utoipa::path]`) and types (`#[derive(ToSchema)]`). The spec is the
//! source of truth for the BE↔FE↔TV contract — generated clients consume it
//! instead of hand-maintained types:
//!   - web:     `bun run gen-api` → openapi-typescript → src/lib/api-types.ts
//!   - Android: openapi-generator (kotlinx-serialization) → TV data layer
//!
//! Emit the spec WITHOUT a running server via the `gen-openapi` bin:
//!   `cargo run -q -p iris-api --bin gen-openapi -- --write`
//!
//! Coverage: the **entire** HTTP surface — auth, devices, search, torrents
//! (incl. the binary stream / HLS / subtitle endpoints), library, follows,
//! me, preferences, playback, for-you, discover, providers, metadata, admin,
//! and the health probe. Hard cases handled: `#[serde(tag = "…")]`
//! discriminated unions (`PollResponse`, `LibraryResponse`) → `oneOf`, and
//! `#[serde(flatten)]` (`SearchResponse`) → `allOf`.
//!
//! To add a new endpoint: annotate the handler with `#[utoipa::path(...)]`,
//! make it (and the request/response DTOs it names) `pub(crate)`, and add the
//! handler to `paths(...)` below. utoipa auto-collects the body/param schemas
//! transitively, so `components(schemas(...))` only needs types that are NOT
//! reachable from a path body (e.g. enums used solely in `IntoParams` query
//! params). The `committed_spec_is_current` test fails until you regenerate
//! (`bun run gen-api`) and commit `web/openapi.json`.

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Iris API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Self-hosted media aggregator + streamer — internal household API.",
    ),
    paths(
        crate::routes::auth::register,
        crate::routes::auth::login,
        crate::routes::auth::refresh,
        crate::routes::auth::logout,
        crate::routes::devices::create_code,
        crate::routes::devices::poll,
        crate::routes::devices::link,
        crate::routes::devices::list,
        crate::routes::devices::revoke,
        crate::routes::search::details,
        crate::routes::search::search,
        crate::routes::torrents::list,
        crate::routes::torrents::get_one,
        crate::routes::torrents::preview,
        crate::routes::torrents::ingest,
        crate::routes::torrents::remove,
        crate::routes::torrents::probe_file,
        crate::routes::torrents::manifest_json,
        crate::routes::torrents::seek_hint,
        crate::routes::torrents::playback_error,
        crate::routes::torrents::play_status,
        crate::routes::torrents::stream_file,
        crate::routes::torrents::play_asset,
        crate::routes::torrents::subtitle_vtt,
        crate::routes::torrents::subtitle_ass,
        crate::routes::torrents::subtitle_sup,
        crate::routes::torrents::get_torrent_progress,
        crate::routes::torrents::get_progress,
        crate::routes::torrents::put_progress,
        crate::routes::library::list_library,
        crate::routes::library::collection_detail,
        crate::routes::library::grab_collection_episode,
        crate::routes::follows::list,
        crate::routes::follows::create,
        crate::routes::follows::remove,
        crate::routes::follows::episodes,
        crate::routes::follows::episode_context,
        crate::routes::follows::grab_episode,
        crate::routes::me::me,
        crate::routes::me::continue_watching,
        crate::routes::me::watchlist,
        crate::routes::me::change_password,
        crate::routes::me::change_display_name,
        crate::routes::preferences::get_preferences,
        crate::routes::preferences::put_preferences,
        crate::routes::preferences::genres,
        crate::routes::preferences::languages,
        crate::routes::playback_preferences::get_prefs,
        crate::routes::playback_preferences::put_prefs,
        crate::routes::foryou::for_you,
        crate::routes::foryou::for_you_page,
        crate::routes::foryou::dismiss,
        crate::routes::discover::featured,
        crate::routes::providers::list,
        crate::routes::metadata::tmdb_lookup,
        crate::routes::metadata::tmdb_search,
        crate::routes::metadata::tmdb_resolve,
        crate::routes::admin::active_sessions,
        crate::routes::admin::watch_history,
        crate::routes::admin::list_users,
        crate::routes::admin::reset_user_password,
        crate::routes::admin::trigger_gc,
        crate::routes::admin::storage_stats,
        crate::routes::admin::list_invitations,
        crate::routes::admin::create_invitation,
        crate::routes::admin::revoke_invitation,
        crate::routes::admin::list_remux_jobs,
        crate::routes::admin::wipe_remux_job,
        crate::routes::admin::diagnose_tmdb,
        crate::routes::health::get,
    ),
    components(schemas(
        crate::routes::auth::RegisterRequest,
        crate::routes::auth::LoginRequest,
        crate::routes::auth::UserResponse,
        crate::routes::devices::CreateCodeRequest,
        crate::routes::devices::CreateCodeResponse,
        crate::routes::devices::LinkRequest,
        crate::routes::devices::DeviceView,
        crate::routes::devices::PollResponse,
        crate::routes::devices::PolledUser,
        iris_core::search::TorrentDetails,
        iris_core::search::MediaInfoSummary,
        iris_core::search::VideoInfo,
        iris_core::search::AudioInfo,
        iris_core::search::SubInfo,
        iris_core::search::DescriptionFormat,
        iris_core::search::SearchResult,
        iris_core::search::MediaKind,
        iris_core::search::SortField,
        iris_core::search::SortOrder,
        iris_providers::registry::AggregatedResults,
        iris_providers::registry::ProviderResultMeta,
        iris_providers::registry::ParsedQueryInfo,
        crate::routes::search::SearchResponse,
        crate::routes::search::LibraryMatch,
        crate::routes::torrents::TorrentView,
        crate::routes::torrents::FileProgressEntry,
        crate::routes::torrents::ProgressView,
        crate::routes::torrents::ProgressUpdate,
        crate::routes::torrents::ResolveBody,
        crate::routes::torrents::IngestResponse,
        crate::routes::torrents::SeekHint,
        crate::routes::torrents::PlaybackErrorBody,
        crate::routes::torrents::PlaybackErrorResponse,
        crate::routes::torrents::PlayStatus,
        iris_torrent::TorrentSnapshot,
        iris_torrent::FileEntry,
        iris_torrent::TorrentState,
        iris_torrent::TorrentPreview,
        iris_torrent::TorrentFilePreview,
        iris_media::MediaProbe,
        iris_media::VideoStream,
        iris_media::AudioStream,
        iris_media::SubtitleStream,
        iris_media::HdrKind,
        iris_media::Manifest,
        iris_media::ByteRange,
        iris_media::DownloadStatus,
        iris_media::VideoTrack,
        iris_media::AudioTrack,
        iris_media::SubtitleTrack,
        iris_media::Chapter,
    )),
    tags(
        (name = "auth", description = "Session auth — register / login / refresh / logout."),
        (name = "devices", description = "Device pairing for headless clients (Android TV)."),
        (name = "search", description = "Tracker search + torrent detail preview."),
        (name = "torrents", description = "Library torrents — list / detail / per-file watch progress."),
        (name = "library", description = "Collections + raw-torrent library views and episode grabs."),
        (name = "follows", description = "Per-user series follows — Watchlist, episode lists, grabs."),
        (name = "me", description = "The authenticated user's own profile, watchlist, continue-watching."),
        (name = "preferences", description = "Recommendation prefs + genre / language vocabularies + playback prefs."),
        (name = "for-you", description = "Personalised recommendation shelves and dismissals."),
        (name = "discover", description = "Featured movie / series carousels aggregated across trackers."),
        (name = "providers", description = "Registered search providers and their capabilities."),
        (name = "metadata", description = "TMDB metadata lookup, typeahead search, SCENE-name resolution."),
        (name = "admin", description = "Admin-only: invitations, users, storage / GC, remux cache, presence, diagnostics."),
        (name = "health", description = "Unauthenticated liveness probe (excluded from the client-version gate)."),
    ),
)]
pub struct ApiDoc;

/// On-disk location of the committed spec (`web/openapi.json`), resolved
/// relative to this crate so it's cwd-independent. The web build regenerates
/// `src/lib/api-types.ts` from it (bun-only, no Rust toolchain), and the
/// snapshot test below keeps it in lockstep with the Rust types.
#[must_use]
pub fn spec_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/openapi.json")
}

/// The spec as pretty JSON — the single rendering the `gen-openapi` bin writes
/// and the snapshot test checks.
#[must_use]
pub fn spec_json() -> String {
    ApiDoc::openapi()
        .to_pretty_json()
        .expect("serialize OpenAPI spec")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fails when `web/openapi.json` is out of date w.r.t. the annotated Rust
    /// types — so a serde/handler change that wasn't re-spec'd can't slip
    /// through `cargo test`. The committed spec is what the web build derives
    /// its types from, so this is the guard that makes Rust changes propagate
    /// automatically. Regenerate with:
    ///   `cargo run -q -p iris-api --bin gen-openapi -- --write` (or `bun run gen-api`)
    #[test]
    fn committed_spec_is_current() {
        let committed = std::fs::read_to_string(spec_path()).expect(
            "read web/openapi.json — run `cargo run -q -p iris-api --bin gen-openapi -- --write`",
        );
        assert!(
            spec_json().trim() == committed.trim(),
            "web/openapi.json is stale vs the Rust types — regenerate with \
             `cargo run -q -p iris-api --bin gen-openapi -- --write` (or `bun run gen-api`) and commit it",
        );
    }
}
