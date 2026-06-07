package studio.kahn.iris.tv.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import retrofit2.http.Body
import retrofit2.http.GET
import retrofit2.http.POST
import retrofit2.http.Path
import retrofit2.http.Query

/**
 * Iris HTTP API surface. Mirrors what the web client uses; we deliberately
 * skip the heavier endpoints (admin, ingest, etc.) until the corresponding
 * TV screens land.
 */
interface IrisApi {

    @POST("api/auth/login")
    suspend fun login(@Body body: LoginRequest): UserResponse

    @POST("api/auth/device/code")
    suspend fun createDeviceCode(@Body body: DeviceCodeRequest): DeviceCodeResponse

    @GET("api/auth/device/poll/{deviceId}")
    suspend fun pollDeviceCode(@Path("deviceId") deviceId: String): DevicePollResponse

    @POST("api/auth/refresh")
    suspend fun refresh(): UserResponse

    @POST("api/auth/logout")
    suspend fun logout()

    @GET("api/me")
    suspend fun me(): UserResponse

    @GET("api/me/continue-watching")
    suspend fun continueWatching(): List<ContinueWatchingItem>

    /** Per-user recommendation preferences (languages / genres / anime).
     *  A never-onboarded user gets the all-empty default with
     *  `onboarding_completed = false`. */
    @GET("api/me/preferences")
    suspend fun preferences(): Preferences

    @retrofit2.http.PUT("api/me/preferences")
    suspend fun savePreferences(@Body body: Preferences): Preferences

    /** Merged movie + TV genre taxonomy for the onboarding picker. Anime
     *  is NOT in here — it's a distinct category driven by
     *  [Preferences.includeAnime]. */
    @GET("api/genres")
    suspend fun genres(): GenresResponse

    /** Server-driven selectable languages for onboarding — fetched so a
     *  new language never requires an APK release. */
    @GET("api/languages")
    suspend fun languages(): LanguagesResponse

    /** The home blended "For You" shelf. */
    @GET("api/me/for-you")
    suspend fun forYou(): ForYouResponse

    /** The organized "For You" page (top picks + per-genre + anime). */
    @GET("api/me/for-you/page")
    suspend fun forYouPage(): ForYouResponse

    /** Hide a recommendation candidate from future shelves. */
    @POST("api/me/for-you/dismiss")
    suspend fun dismissForYou(@Body body: DismissForYouRequest)

    /** The user's preferred audio + subtitle language (applied across episodes
     *  / devices). */
    @GET("api/me/playback-preferences")
    suspend fun playbackPreferences(): PlaybackPrefs

    /** Save preferred audio + subtitle language. Send the full current state. */
    @retrofit2.http.PUT("api/me/playback-preferences")
    suspend fun savePlaybackPreferences(@Body body: PlaybackPrefs)

    @GET("api/torrents")
    suspend fun listTorrents(): List<TorrentView>

    @GET("api/torrents/{infohash}")
    suspend fun getTorrent(@Path("infohash") infohash: String): TorrentView

    /** Remove a torrent: the backend also wipes its files from disk and
     *  soft-deletes the row (204 No Content). Any authenticated user —
     *  the seedbox view is single-household, same as the web client. */
    @retrofit2.http.DELETE("api/torrents/{infohash}")
    suspend fun deleteTorrent(@Path("infohash") infohash: String)

    @GET("api/torrents/{infohash}/progress")
    suspend fun torrentProgress(@Path("infohash") infohash: String): List<FileProgressEntry>

    @GET("api/torrents/{infohash}/files/{idx}/probe")
    suspend fun probe(
        @Path("infohash") infohash: String,
        @Path("idx") idx: Int,
    ): MediaProbe

    /**
     * Capability-negotiated playback manifest. Superset of [probe] — adds
     * MSE/WebCodecs codec strings, HDR metadata, container index layout,
     * and per-track sidecar URLs. See `docs/SOTA_ARCHITECTURE.md` §2.1.
     *
     * Phase 0 of the Android TV client keeps using `/play/master.m3u8`
     * for actual playback; this call exists to validate the wire contract
     * and seed Phase 3's switch to direct-blob playback.
     */
    @GET("api/torrents/{infohash}/files/{idx}/manifest.json")
    suspend fun manifest(
        @Path("infohash") infohash: String,
        @Path("idx") idx: Int,
    ): Manifest

    @GET("api/torrents/{infohash}/files/{idx}/play/status")
    suspend fun playStatus(
        @Path("infohash") infohash: String,
        @Path("idx") idx: Int,
    ): PlayStatus

    @retrofit2.http.PUT("api/torrents/{infohash}/files/{idx}/progress")
    suspend fun saveProgress(
        @Path("infohash") infohash: String,
        @Path("idx") idx: Int,
        @Body body: ProgressUpdate,
    )

    /** Single-file progress with `audio_track_idx` + `subtitle_track_idx`.
     *  Returns `null` when no entry exists yet (first watch). */
    @GET("api/torrents/{infohash}/files/{idx}/progress")
    suspend fun getProgress(
        @Path("infohash") infohash: String,
        @Path("idx") idx: Int,
    ): ProgressView?

    /** Fire-and-forget hint: the user just seeked to `playheadSec`. The
     *  server bias-prioritises ~30 s of pieces forward of the matching
     *  byte offset (`prefetch_range`) so the subsequent Range request
     *  from Media3 doesn't sit on undownloaded pieces. Best-effort:
     *  returns 204 and the prefetch runs in the background. Web does
     *  the equivalent via `postSeekHint`; TV used to skip it, which is
     *  fine on a quick Android TV stack but bites on Fire TV where the
     *  client-side cancel + reconnect can race librqbit's piece picker. */
    @POST("api/torrents/{infohash}/files/{idx}/seek")
    suspend fun postSeekHint(
        @Path("infohash") infohash: String,
        @Path("idx") idx: Int,
        @Body body: SeekHint,
    )

    /** TMDB id lookup. `kind` disambiguates the movie/tv namespaces —
     *  same numerical id resolves to two unrelated entries otherwise.
     *  Pass the collection / search-result kind whenever known. */
    @GET("api/metadata/tmdb/{id}")
    suspend fun tmdbMetadata(
        @Path("id") id: Long,
        @Query("kind") kind: String? = null,
    ): TmdbMetadata

    /** TMDB multi-search (movies + tv). Powers the live suggestion panel
     *  on the search screen, mirrors what the web UI uses. Empty list on
     *  network / config failure (server returns `[]`, never errors). */
    @GET("api/metadata/tmdb/search")
    suspend fun tmdbSearch(@Query("q") q: String): List<TmdbSuggestion>

    /** Resolve a raw release title to its single best TMDB match. Scored
     *  server-side by kind + year (not popularity) and served from the
     *  persistent 30d resolve cache — this is the poster path for search
     *  results. Send the untouched release name; the backend parses
     *  title/year/kind out of it (one source of truth instead of a
     *  per-client SCENE parser). `null` when nothing matched / TMDB
     *  unconfigured. */
    @GET("api/metadata/tmdb/resolve")
    suspend fun tmdbResolve(
        @Query("title") title: String,
        @Query("kind") kind: String? = null,
    ): TmdbSuggestion?

    @GET("api/search")
    suspend fun search(
        @Query("q") q: String,
        @Query("page") page: Int? = null,
        @Query("limit") limit: Int? = null,
        @Query("sort_by") sortBy: String? = null,
        @Query("order") order: String? = null,
        @Query("kind") kind: String? = null,
    ): AggregatedResults

    @GET("api/search/details")
    suspend fun torrentDetails(
        @Query("provider") provider: String,
        @Query("id") id: String,
    ): TorrentDetails

    /** Add a torrent to Iris from a search hit. Returns the snapshot + id. */
    @POST("api/torrents")
    suspend fun ingest(@Body body: IngestRequest): IngestResponse

    @GET("api/me/devices")
    suspend fun listDevices(): List<DeviceView>

    @retrofit2.http.DELETE("api/me/devices/{jti}")
    suspend fun revokeDevice(@Path("jti") jti: String)

    // ----------- Discovery + series follows (Phase 2 / Phase 4) -----------

    @GET("api/discover/featured")
    suspend fun discoverFeatured(): FeaturedResponse

    @GET("api/me/follows")
    suspend fun listFollows(): List<FollowSummary>

    @POST("api/me/follows")
    suspend fun addFollow(@Body body: AddFollowRequest): FollowSummary

    @retrofit2.http.DELETE("api/me/follows/{id}")
    suspend fun removeFollow(@Path("id") id: String)

    @GET("api/me/follows/{id}/episodes")
    suspend fun followEpisodes(
        @Path("id") id: String,
        @Query("season") season: Int? = null,
    ): EpisodesResponse

    @POST("api/me/follows/{id}/episodes/{season}/{episode}/grab")
    suspend fun grabEpisode(
        @Path("id") id: String,
        @Path("season") season: Int,
        @Path("episode") episode: Int,
    ): GrabEpisodeResponse

    @GET("api/me/follows/episode-context")
    suspend fun episodeContext(
        @Query("infohash") infohash: String,
        @Query("file_idx") fileIdx: Int,
    ): EpisodeContext

    @GET("api/library")
    suspend fun library(@Query("view") view: String = "collections"): LibraryResponse

    /** Full content of a single collection — every torrent attached
     *  plus the merged episode list (TV only). Powers the
     *  `CollectionScreen` browse view. */
    @GET("api/library/collections/{id}")
    suspend fun collectionDetail(@Path("id") id: String): CollectionDetail

    /** Per-user Watchlist (post-0.4) — TV collections the calling
     *  user has at least one episode of, sourced from their
     *  auto-created `series_follows` rows. The legacy
     *  `listFollows()` above still works through the C1 façade for
     *  APK 0.3.1 clients; new code should prefer this. */
    @GET("api/me/watchlist")
    suspend fun watchlist(): List<WatchlistItem>

    /** Grab a specific (season, episode) by collection id. With
     *  `language` set, the server picks strictly from that
     *  language slot — no cross-language fallback. Used when the
     *  user clicked an FR / EN badge on a multi-language row. */
    @POST("api/library/collections/{id}/grab/{season}/{episode}")
    suspend fun grabCollectionEpisode(
        @Path("id") id: String,
        @Path("season") season: Int,
        @Path("episode") episode: Int,
        @Query("language") language: String? = null,
    ): GrabEpisodeResponse
}

// ============================== DTOs ====================================

@Serializable
data class LoginRequest(val email: String, val password: String)

@Serializable
data class DeviceCodeRequest(val kind: String = "android-tv")

@Serializable
data class DeviceCodeResponse(
    val code: String,
    @SerialName("device_id") val deviceId: String,
    @SerialName("verification_url") val verificationUrl: String,
    @SerialName("expires_in") val expiresIn: Long,
)

@Serializable
data class DevicePollResponse(
    val status: String,                  // "pending" | "linked" | "expired"
    val user: UserResponse? = null,
)

@Serializable
data class UserResponse(
    val id: String,
    val email: String,
    @SerialName("display_name") val displayName: String = "",
    @SerialName("is_admin") val isAdmin: Boolean,
)

/**
 * Per-user recommendation preferences (Slice 1 of "For You"). Every field
 * defaults so an older server (no preferences endpoint) or a partial
 * payload deserialises cleanly — `languages` uses the backend `Language`
 * vocabulary ("french" / "english"); `genres` holds TMDB genre ids;
 * `includeAnime` is the distinct Anime category (NOT TMDB "Animation"),
 * later fed by the AniList pipeline.
 */
@Serializable
data class Preferences(
    val languages: List<String> = emptyList(),
    val genres: List<Long> = emptyList(),
    @SerialName("include_anime") val includeAnime: Boolean = false,
    @SerialName("onboarding_completed") val onboardingCompleted: Boolean = false,
)

@Serializable
data class GenreOption(
    val id: Long,
    val name: String = "",
)

@Serializable
data class GenresResponse(
    val genres: List<GenreOption> = emptyList(),
)

@Serializable
data class LanguageOption(
    val value: String,
    val label: String = "",
)

@Serializable
data class LanguagesResponse(
    val languages: List<LanguageOption> = emptyList(),
)

/** A recommendation candidate as rendered on a "For You" shelf. Posters
 *  are pre-resolved server-side (TMDB CDN or AniList cover), so render
 *  [posterUrl] directly. `new_count`/`collection_id` are set on
 *  "new episodes" cards (a followed series with new content). */
@Serializable
data class CatalogCard(
    @SerialName("catalog_id") val catalogId: String,
    @SerialName("tmdb_id") val tmdbId: Long? = null,
    val kind: String = "movie",
    val title: String = "",
    @SerialName("poster_url") val posterUrl: String? = null,
    @SerialName("backdrop_url") val backdropUrl: String? = null,
    val overview: String? = null,
    @SerialName("is_anime") val isAnime: Boolean = false,
    val availability: String = "unknown",
    /** Seeder count of the recorded release (rolling-window rows). null for
     *  lazy reco candidates / torr9 RSS rows (re-checked at grab). */
    val seeders: Int? = null,
    /** The recorded release's provider + id — lets the TV open the same
     *  detail/preview screen as a search hit. null for lazy reco candidates
     *  (the TV falls back to a title search). */
    @SerialName("provider_id") val providerId: String? = null,
    @SerialName("external_id") val externalId: String? = null,
    val year: Int? = null,
    @SerialName("already_in_library") val alreadyInLibrary: Boolean = false,
    @SerialName("library_infohash") val libraryInfohash: String? = null,
    @SerialName("new_count") val newCount: Long? = null,
    @SerialName("collection_id") val collectionId: String? = null,
)

@Serializable
data class ForYouShelf(
    val key: String = "",
    val title: String = "",
    val kind: String? = null,
    val items: List<CatalogCard> = emptyList(),
)

@Serializable
data class ForYouResponse(
    val shelves: List<ForYouShelf> = emptyList(),
)

@Serializable
data class DismissForYouRequest(@SerialName("catalog_id") val catalogId: String)

/** Per-user playback language preferences (GET response + PUT body).
 *  `subtitleLanguage == "off"` means subtitles disabled; null = no
 *  preference. Volume isn't here — Android TV uses the system volume. */
@Serializable
data class PlaybackPrefs(
    @SerialName("audio_language") val audioLanguage: String? = null,
    @SerialName("subtitle_language") val subtitleLanguage: String? = null,
)

@Serializable
data class ContinueWatchingItem(
    val infohash: String,
    @SerialName("torrent_name") val torrentName: String,
    @SerialName("tmdb_id") val tmdbId: Long? = null,
    @SerialName("tmdb_verified") val tmdbVerified: Boolean = false,
    /** `"movie"` | `"tv"` from the parent collection. Passed to the
     *  TMDB lookup so the right namespace is hit (movies and TV
     *  shows have separate id spaces; the same numerical id resolves
     *  to two unrelated entries otherwise). */
    val kind: String? = null,
    @SerialName("file_idx") val fileIdx: Int,
    @SerialName("file_path") val filePath: String? = null,
    @SerialName("position_seconds") val positionSeconds: Double,
    @SerialName("duration_seconds") val durationSeconds: Double? = null,
    @SerialName("last_watched_at") val lastWatchedAt: String,
    val completed: Boolean,
)

@Serializable
data class FileEntry(
    val index: Int,
    val path: String,
    @SerialName("size_bytes") val sizeBytes: Long,
)

@Serializable
data class TorrentView(
    val id: String,
    val infohash: String,
    val name: String? = null,
    @SerialName("tmdb_id") val tmdbId: Long? = null,
    @SerialName("tmdb_verified") val tmdbVerified: Boolean = false,
    @SerialName("added_by_name") val addedByName: String = "",
    @SerialName("added_at") val addedAt: String = "",
    @SerialName("total_size_bytes") val totalSizeBytes: Long,
    val state: String,
    @SerialName("progress_bytes") val progressBytes: Long,
    @SerialName("progress_pct") val progressPct: Float,
    @SerialName("download_speed_bps") val downloadSpeedBps: Long,
    @SerialName("upload_speed_bps") val uploadSpeedBps: Long,
    val peers: Int,
    val files: List<FileEntry> = emptyList(),
    val error: String? = null,
    /** Lifetime upload counter — survives session restarts and GC.
     *  Default 0 keeps backward-compat with older servers. */
    @SerialName("uploaded_bytes_total") val uploadedBytesTotal: Long = 0,
    /** `"movie"` | `"tv"` from the parent collection. Pass to TMDB
     *  lookups to avoid namespace-id collisions. */
    val kind: String? = null,
)

@Serializable
data class FileProgressEntry(
    @SerialName("file_idx") val fileIdx: Int,
    @SerialName("position_seconds") val positionSeconds: Double,
    @SerialName("duration_seconds") val durationSeconds: Double? = null,
    val completed: Boolean,
    @SerialName("last_watched_at") val lastWatchedAt: String,
)

@Serializable
data class AudioStream(
    val index: Int,
    val codec: String,
    val channels: Int,
    val language: String? = null,
    val title: String? = null,
    val default: Boolean = false,
)

@Serializable
data class SubtitleStream(
    val index: Int,
    val codec: String,
    val language: String? = null,
    val title: String? = null,
    val default: Boolean = false,
    val forced: Boolean = false,
    @SerialName("text_based") val textBased: Boolean,
)

@Serializable
data class VideoStream(
    val index: Int,
    val codec: String,
    val width: Int? = null,
    val height: Int? = null,
)

@Serializable
data class MediaProbe(
    val container: String,
    @SerialName("duration_seconds") val durationSeconds: Double? = null,
    val video: List<VideoStream>,
    val audio: List<AudioStream>,
    val subtitle: List<SubtitleStream>,
)

// ----- Manifest (Phase 0 of the capability-negotiated pipeline) -----------

@Serializable
data class ByteRange(val start: Long, val end: Long)

@Serializable
data class DownloadStatus(
    val progress: Double,
    @SerialName("ranges_complete") val rangesComplete: List<List<Long>> = emptyList(),
    @SerialName("bytes_complete") val bytesComplete: Long = 0,
)

@Serializable
data class ManifestVideoTrack(
    @SerialName("stream_idx") val streamIdx: Int,
    val codec: String,
    @SerialName("codec_string") val codecString: String? = null,
    val profile: String? = null,
    val level: Int? = null,
    @SerialName("bit_depth") val bitDepth: Int? = null,
    val width: Int? = null,
    val height: Int? = null,
    @SerialName("fps_num") val fpsNum: Int? = null,
    @SerialName("fps_den") val fpsDen: Int? = null,
    /** "none" | "hdr10" | "hdr10_plus" | "dovi" | "hlg" */
    val hdr: String = "none",
    @SerialName("color_primaries") val colorPrimaries: String? = null,
    @SerialName("color_transfer") val colorTransfer: String? = null,
    @SerialName("color_matrix") val colorMatrix: String? = null,
    @SerialName("max_cll") val maxCll: Int? = null,
    @SerialName("max_fall") val maxFall: Int? = null,
)

@Serializable
data class ManifestAudioTrack(
    @SerialName("stream_idx") val streamIdx: Int,
    val codec: String,
    @SerialName("codec_string") val codecString: String? = null,
    val channels: Int,
    @SerialName("channel_layout") val channelLayout: String? = null,
    @SerialName("sample_rate") val sampleRate: Int? = null,
    val bitrate: Long? = null,
    val lang: String? = null,
    val title: String? = null,
    val default: Boolean = false,
    val forced: Boolean = false,
    @SerialName("browser_native") val browserNative: Boolean = false,
)

@Serializable
data class ManifestSubtitleTrack(
    @SerialName("stream_idx") val streamIdx: Int,
    val codec: String,
    val lang: String? = null,
    val title: String? = null,
    val default: Boolean = false,
    val forced: Boolean = false,
    @SerialName("text_based") val textBased: Boolean,
    val extractable: Boolean = true,
    val url: String,
)

@Serializable
data class ManifestChapter(
    @SerialName("start_s") val startS: Double,
    @SerialName("end_s") val endS: Double,
    val title: String? = null,
)

@Serializable
data class Manifest(
    @SerialName("schema_version") val schemaVersion: Int,
    val infohash: String,
    @SerialName("file_idx") val fileIdx: Int,
    val filename: String,
    val container: String,
    @SerialName("duration_s") val durationS: Double? = null,
    @SerialName("size_bytes") val sizeBytes: Long,
    @SerialName("moov_at_start") val moovAtStart: Boolean? = null,
    @SerialName("index_at_end") val indexAtEnd: Boolean = true,
    @SerialName("header_byte_range") val headerByteRange: ByteRange,
    @SerialName("tail_byte_range") val tailByteRange: ByteRange? = null,
    val download: DownloadStatus,
    val video: List<ManifestVideoTrack> = emptyList(),
    val audio: List<ManifestAudioTrack> = emptyList(),
    val subtitles: List<ManifestSubtitleTrack> = emptyList(),
    val chapters: List<ManifestChapter> = emptyList(),
)

/**
 * Pre-mount loading telemetry for the playback pipeline. Mirrors the
 * `PlayStatus` shape returned by `/torrents/{ih}/files/{idx}/play/status`.
 *
 * `reason` is one of `"downloading"` | `"remuxing"` | `"preparing"` while
 * the file isn't yet ready, and `null` once `ready == true`. `progress`
 * is a 0..1 fraction populated for both `"downloading"` (torrent
 * progress) and `"remuxing"` (ffmpeg encoded position over total
 * duration). May be null briefly while the relevant source is still
 * publishing its first measurement.
 */
@Serializable
data class PlayStatus(
    val ready: Boolean,
    val reason: String? = null,
    val progress: Float? = null,
    val error: String? = null,
)

@Serializable
data class ProgressUpdate(
    @SerialName("position_seconds") val positionSeconds: Double,
    @SerialName("duration_seconds") val durationSeconds: Double? = null,
    @SerialName("audio_track_idx") val audioTrackIdx: Int? = null,
    @SerialName("subtitle_track_idx") val subtitleTrackIdx: Int? = null,
    val completed: Boolean = false,
)

/** Wire format for `POST /api/torrents/.../seek`. `playhead_s` is
 *  optional but recommended — the server uses it for telemetry; the
 *  authoritative piece-priority bias is `byte_offset`. */
@Serializable
data class SeekHint(
    @SerialName("byte_offset") val byteOffset: Long,
    @SerialName("playhead_s") val playheadS: Double? = null,
)

/**
 * Single-file playback progress, the GET counterpart of
 * [ProgressUpdate]. Mirrors `ProgressView` in `iris-api`. Returned by
 * `getProgress(infohash, idx)`, used at mount time to restore the
 * user's last audio + subtitle picks (the position is restored from
 * the bulk endpoint already).
 */
@Serializable
data class ProgressView(
    @SerialName("position_seconds") val positionSeconds: Double,
    @SerialName("duration_seconds") val durationSeconds: Double? = null,
    @SerialName("audio_track_idx") val audioTrackIdx: Int? = null,
    @SerialName("subtitle_track_idx") val subtitleTrackIdx: Int? = null,
    val completed: Boolean,
)

/**
 * One TMDB typeahead suggestion. Mirrors `TmdbSuggestion` on the web /
 * server side. `kind` is `"movie"` or `"tv"` — drives the kind chip and
 * pre-filters the indexer search when the user picks the suggestion.
 */
@Serializable
data class TmdbSuggestion(
    val kind: String,
    @SerialName("tmdb_id") val tmdbId: Long,
    val title: String,
    val year: Int? = null,
    val overview: String? = null,
    @SerialName("poster_path") val posterPath: String? = null,
)

@Serializable
data class TmdbMetadata(
    val kind: String,
    @SerialName("tmdb_id") val tmdbId: Long,
    val title: String,
    val overview: String? = null,
    val year: Int? = null,
    @SerialName("poster_path") val posterPath: String? = null,
    @SerialName("backdrop_path") val backdropPath: String? = null,
    @SerialName("vote_score") val voteScore: Float? = null,
    val genres: List<String> = emptyList(),
    @SerialName("runtime_minutes") val runtimeMinutes: Int? = null,
    @SerialName("number_of_seasons") val numberOfSeasons: Int? = null,
)

@Serializable
data class SearchResult(
    @SerialName("provider_id") val providerId: String,
    @SerialName("external_id") val externalId: String,
    val title: String,
    val year: Int? = null,
    @SerialName("size_bytes") val sizeBytes: Long? = null,
    val seeders: Int? = null,
    val leechers: Int? = null,
    val infohash: String? = null,
    val category: String? = null,
    val tags: List<String> = emptyList(),
    val freeleech: Boolean = false,
    @SerialName("tmdb_id") val tmdbId: Long? = null,
    val kind: String? = null,
    /** Pre-resolved poster URL when the indexer ships one (torr9
     *  includes it on featured items). Use directly — skips the
     *  strict tmdb_verified gate that would otherwise hide every
     *  poster on the discovery shelf. */
    @SerialName("poster_url") val posterUrl: String? = null,
    /** Server-side library dedup: result's SCENE identity already
     *  maps to an episode_files row. UI surfaces an "already
     *  downloaded" pill and offers to play the existing file
     *  rather than re-downloading. Defaults to false so APKs
     *  built against backends that don't ship this field keep
     *  working. */
    @SerialName("already_in_library") val alreadyInLibrary: Boolean = false,
    @SerialName("library_infohash") val libraryInfohash: String? = null,
    @SerialName("library_file_idx") val libraryFileIdx: Int? = null,
    /** Server-detected language tag (`french` / `english` / `multi` /
     *  `unknown`). Future TV releases render an FR / EN badge per
     *  card; 0.3.x ignores it silently (ignoreUnknownKeys). */
    val language: String? = null,
)

@Serializable
data class ProviderResultMeta(
    val id: String,
    @SerialName("current_page") val currentPage: Int,
    val limit: Int,
    @SerialName("total_count") val totalCount: Long? = null,
    @SerialName("total_pages") val totalPages: Int? = null,
    val error: String? = null,
)

/** SCENE-style breakdown of the user query the backend ran before
 *  fan-out — title + season + episode + year. The TV `SearchScreen`
 *  can render this as a "Showing results for X · S04E11" header to
 *  reassure the user the indexer was queried with the structured
 *  fields. Null when the parser saw nothing useful. */
@Serializable
data class ParsedQueryInfo(
    val title: String,
    val season: Int? = null,
    val episode: Int? = null,
    val year: Int? = null,
)

@Serializable
data class AggregatedResults(
    val results: List<SearchResult>,
    val providers: List<ProviderResultMeta>,
    @SerialName("parsed_query") val parsedQuery: ParsedQueryInfo? = null,
)

@Serializable
data class IngestRequest(
    @SerialName("provider_id") val providerId: String,
    @SerialName("external_id") val externalId: String,
    @SerialName("tmdb_id") val tmdbId: Long? = null,
)

/**
 * The `snapshot` field of `IngestResponse` is a [`TorrentSnapshot`][crates.iris_torrent.engine]
 * — runtime state from the librqbit engine, *without* the database-side
 * fields (`id`, `added_by`, `added_at`, `tmdb_id`). Those live next to it on
 * `TorrentView` (and on the top level of `IngestResponse`).
 *
 * Modelling this with a separate type instead of reusing `TorrentView`
 * avoids the "Field 'id' is required …" deserialization error we'd
 * otherwise hit on every ingest.
 */
@Serializable
data class TorrentSnapshot(
    val infohash: String,
    val name: String? = null,
    @SerialName("total_size_bytes") val totalSizeBytes: Long,
    val state: String,
    @SerialName("progress_bytes") val progressBytes: Long,
    @SerialName("progress_pct") val progressPct: Float,
    @SerialName("download_speed_bps") val downloadSpeedBps: Long,
    @SerialName("upload_speed_bps") val uploadSpeedBps: Long,
    val peers: Int = 0,
    val files: List<FileEntry> = emptyList(),
    val error: String? = null,
    val finished: Boolean = false,
)

@Serializable
data class IngestResponse(
    val id: String,
    @SerialName("already_managed") val alreadyManaged: Boolean,
    val snapshot: TorrentSnapshot,
)

@Serializable
data class DeviceView(
    val jti: String,
    val label: String? = null,
    val kind: String? = null,
    @SerialName("issued_at") val issuedAt: String,
    @SerialName("expires_at") val expiresAt: String,
)

@Serializable
data class FeaturedResponse(
    val movies: List<SearchResult> = emptyList(),
    val series: List<SearchResult> = emptyList(),
)

@Serializable
data class FollowSummary(
    /** Stable id — clients route by this, not by tmdbId. */
    val id: String,
    /** SCENE-normalised name (lowercased, punctuation-stripped, single
     *  spaces). Identity for joining episode_files / available_episodes. */
    @SerialName("normalized_name") val normalizedName: String,
    val name: String,
    /** Decoration only. Server gates poster/backdrop paths on the joined
     *  collection being tmdb_verified — null poster = no verified match. */
    @SerialName("tmdb_id") val tmdbId: Long? = null,
    @SerialName("poster_path") val posterPath: String? = null,
    @SerialName("backdrop_path") val backdropPath: String? = null,
    /** Number of distinct (S, E) the indexer has surfaced since
     *  lastVisitedAt. Drives the "X new" badge on Watchlist cards. */
    @SerialName("new_count") val newCount: Long = 0,
    @SerialName("last_visited_at") val lastVisitedAt: String? = null,
    @SerialName("created_at") val createdAt: String,
)

/** Post-0.4 Watchlist tile — returned by `/api/me/watchlist`.
 *  Per-user: derived from the calling user's `series_follows` rows
 *  (auto-created on grab). `id` is a collection id when one
 *  already exists for this normalised name, otherwise the follow
 *  row's own id — either form routes to `CollectionScreen`. */
@Serializable
data class WatchlistItem(
    val id: String,
    @SerialName("normalized_name") val normalizedName: String,
    val name: String,
    @SerialName("tmdb_id") val tmdbId: Long? = null,
    @SerialName("poster_path") val posterPath: String? = null,
    @SerialName("backdrop_path") val backdropPath: String? = null,
    @SerialName("new_count") val newCount: Long = 0,
    @SerialName("last_visited_at") val lastVisitedAt: String? = null,
    @SerialName("created_at") val createdAt: String,
)

/**
 * Encoding of [TorrentDetails.description]. Mirrors the Rust
 * `iris_core::search::DescriptionFormat` enum (lowercase serde). Defaults
 * to [BBCODE] when the server omits the field — older payloads predate
 * c411 and were always BBCode (torr9).
 */
@Serializable
enum class DescriptionFormat {
    @SerialName("bbcode") BBCODE,
    @SerialName("html") HTML,
    @SerialName("plain") PLAIN,
}

@Serializable
data class TorrentDetails(
    @SerialName("provider_id") val providerId: String,
    @SerialName("external_id") val externalId: String,
    val title: String,
    val description: String? = null,
    @SerialName("description_format")
    val descriptionFormat: DescriptionFormat = DescriptionFormat.BBCODE,
    val nfo: String? = null,
    @SerialName("media_info") val mediaInfo: MediaInfoSummary? = null,
    val tags: List<String> = emptyList(),
    val category: String? = null,
    val uploader: String? = null,
    @SerialName("uploaded_at") val uploadedAt: String? = null,
    val age: String? = null,
    val seeders: Int? = null,
    val leechers: Int? = null,
    @SerialName("times_completed") val timesCompleted: Long? = null,
    val views: Long? = null,
    val freeleech: Boolean = false,
    val exclusive: Boolean = false,
    @SerialName("file_count") val fileCount: Int? = null,
    @SerialName("file_size_bytes") val fileSizeBytes: Long? = null,
)

@Serializable
data class MediaInfoSummary(
    val video: VideoInfoDetails? = null,
    val audio: List<AudioInfoDetails> = emptyList(),
    val subtitles: List<SubInfoDetails> = emptyList(),
)

@Serializable
data class VideoInfoDetails(
    val codec: String? = null,
    val resolution: String? = null,
    @SerialName("duration_secs") val durationSecs: Int? = null,
    val fps: Float? = null,
    @SerialName("bitrate_kbps") val bitrateKbps: Int? = null,
    val hdr: String? = null,
)

@Serializable
data class AudioInfoDetails(
    val lang: String? = null,
    val codec: String? = null,
    val channels: Int? = null,
    @SerialName("bitrate_kbps") val bitrateKbps: Int? = null,
    val title: String? = null,
    val default: Boolean = false,
    @SerialName("commercial_name") val commercialName: String? = null,
)

@Serializable
data class SubInfoDetails(
    val lang: String? = null,
    val format: String? = null,
    val title: String? = null,
    val default: Boolean = false,
    val forced: Boolean = false,
)

@Serializable
data class AddFollowRequest(
    /** Display name from whatever surface the user clicked. Server
     *  derives normalized_name itself. */
    val name: String,
    /** Optional decoration. Server stores it but only renders a
     *  poster after the joined collection is tmdb_verified. */
    @SerialName("tmdb_id") val tmdbId: Long? = null,
)

@Serializable
data class EpisodesResponse(
    /** Echoes the request's season filter — null when the caller
     *  asked for all seasons. */
    val season: Int? = null,
    val items: List<EpisodeItem> = emptyList(),
)

@Serializable
data class EpisodeItem(
    val season: Int,
    val episode: Int,
    /** "downloaded" | "available" */
    val status: String,
    val watched: Boolean = false,
    val infohash: String? = null,
    @SerialName("file_idx") val fileIdx: Int? = null,
    @SerialName("indexer_provider") val indexerProvider: String? = null,
    @SerialName("indexer_torrent_id") val indexerTorrentId: String? = null,
    val quality: String? = null,
    val seeders: Long? = null,
)

@Serializable
data class GrabEpisodeResponse(
    val infohash: String,
    @SerialName("file_idx") val fileIdx: Int,
    @SerialName("already_grabbed") val alreadyGrabbed: Boolean,
)

@Serializable
data class EpisodeContext(
    val followed: Boolean,
    val current: EpisodePoint? = null,
    val next: EpisodePoint? = null,
    /** Previous episode (symmetric to [next]). The TV player exposes
     *  a "‹ Prev" chip alongside the next-episode chip so the user
     *  can step back into the last watched episode without going
     *  through the Series detail screen. */
    val prev: EpisodePoint? = null,
)

@Serializable
data class EpisodePoint(
    /** Follow id used to drive the on-demand grab call. Null when
     *  the file isn't part of a followed series. */
    @SerialName("follow_id") val followId: String? = null,
    val season: Int,
    val episode: Int,
    /** "downloaded" | "available" */
    val status: String,
    /** Physical infohash for `status == "downloaded"`. Lets WatchScreen
     *  navigate straight to the next episode without re-grabbing. Null
     *  when only an indexer cache entry exists. */
    val infohash: String? = null,
    /** File index inside [infohash] for `status == "downloaded"`. */
    @SerialName("file_idx") val fileIdx: Int? = null,
)

/**
 * Polymorphic response from `/api/library?view=...`. The backend
 * tags each variant with the `view` field (`#[serde(tag = "view")]`
 * on the Rust side), so we override kotlinx.serialization's default
 * `"type"` discriminator to match. Without this, deserialisation
 * silently fails — no Library shelf, even though the API returned
 * a perfectly good payload.
 */
@OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)
@Serializable
@kotlinx.serialization.json.JsonClassDiscriminator("view")
sealed class LibraryResponse {
    @Serializable
    @SerialName("collections")
    data class Collections(val items: List<CollectionListItem> = emptyList()) : LibraryResponse()

    @Serializable
    @SerialName("torrents")
    data class Torrents(
        val items: List<TorrentView> = emptyList(),
        @SerialName("total_uploaded_bytes") val totalUploadedBytes: Long = 0,
    ) : LibraryResponse()
}

@Serializable
data class CollectionDetail(
    val id: String,
    @SerialName("tmdb_id") val tmdbId: Long? = null,
    @SerialName("display_title") val displayTitle: String,
    val kind: String,
    /** True for anime collections (AniList-enriched, split from a
     *  same-titled live-action show). Defaulted for forward-compat. */
    @SerialName("is_anime") val isAnime: Boolean = false,
    /** Episode layout: `"seasonal"` (season tabs, default) or
     *  `"absolute"` (one flat "Episode N" list — fleuve anime).
     *  Derived server-side from the episode data, NOT from `isAnime`.
     *  Defaulted so older servers / payloads keep the seasonal layout. */
    val numbering: String = "seasonal",
    val torrents: List<TorrentView> = emptyList(),
    val episodes: List<CollectionEpisode> = emptyList(),
    /** Server-resolved TMDB poster/backdrop (TMDB convention,
     *  `/abc123.jpg`). Saves a separate `tmdbMetadata` round-trip
     *  on every CollectionScreen open. Null when the collection
     *  has no tmdb_id or the TMDB lookup failed. */
    @SerialName("poster_path") val posterPath: String? = null,
    @SerialName("backdrop_path") val backdropPath: String? = null,
    /** Indexer-cached "grabbable next episodes" for this collection.
     *  Server filters out (S, E) already in `episodes` so this list
     *  is genuinely "what you could download". One row per
     *  (S, E, language) — anglophone + francophone users see their
     *  badges side by side. Empty for movies / no-SCENE collections. */
    @SerialName("available_episodes") val availableEpisodes: List<AvailableEpisodeEntry> = emptyList(),
    /** Cached season-pack offers (separate from per-episode rows).
     *  CollectionScreen renders one "Grab full Season N" CTA per
     *  pack/language; the grab path also consults these
     *  transparently when a user clicks a (S, E) with no singleton
     *  offer. */
    @SerialName("season_packs") val seasonPacks: List<SeasonPackEntry> = emptyList(),
    /** Count of `available_episodes` whose `found_at >
     *  last_visited_at` (per-user). Drives a "X new" hero badge
     *  on the CollectionScreen. */
    @SerialName("has_new_since_last_visit") val hasNewSinceLastVisit: Long = 0,
)

@Serializable
data class CollectionEpisode(
    val season: Long,
    val episode: Long,
    val infohash: String,
    @SerialName("file_idx") val fileIdx: Int,
    val watched: Boolean = false,
    /** Server-detected language tag (`french` / `english` /
     *  `multi` / `unknown`). Rendered as a row badge — every TV
     *  row, downloaded or not, carries one. `null` only when the
     *  parent torrent is no longer registered (defensive). */
    val language: String? = null,
    /** Absolute episode number for fleuve anime (`One Piece S01E1156`
     *  → 1156). `null` for seasonal episodes. Rendered as "Episode N"
     *  when the collection's `numbering` is `"absolute"`. */
    @SerialName("absolute_episode") val absoluteEpisode: Long? = null,
)

/** Indexer offer the scheduler cached for an episode that isn't
 *  yet on disk. The CollectionScreen renders one row per entry
 *  with a `LanguageBadge`; clicking grabs that specific language
 *  variant. */
/** Cached season-pack offer the scheduler stashed for this
 *  collection. One per (season, language) — the
 *  `CollectionScreen` renders a "Grab full Season N" CTA per
 *  entry. The grab path also consults these as a fallback when a
 *  user clicks a (S, E) with no singleton offer. */
@Serializable
data class SeasonPackEntry(
    val season: Long,
    @SerialName("indexer_provider") val indexerProvider: String,
    @SerialName("indexer_torrent_id") val indexerTorrentId: String,
    val quality: String? = null,
    val seeders: Long? = null,
    @SerialName("size_bytes") val sizeBytes: Long? = null,
    @SerialName("found_at") val foundAt: String,
    val language: String? = null,
)

@Serializable
data class AvailableEpisodeEntry(
    val season: Long,
    val episode: Long,
    @SerialName("indexer_provider") val indexerProvider: String,
    @SerialName("indexer_torrent_id") val indexerTorrentId: String,
    val quality: String? = null,
    val seeders: Long? = null,
    @SerialName("size_bytes") val sizeBytes: Long? = null,
    @SerialName("found_at") val foundAt: String,
    /** `"french"` / `"english"` / `"multi"` / `"unknown"`. Drives
     *  the FR / EN / MULTi pill on the episode row. */
    val language: String? = null,
    /** Absolute episode number for fleuve anime offers. `null` for
     *  seasonal releases. */
    @SerialName("absolute_episode") val absoluteEpisode: Long? = null,
)

@Serializable
data class CollectionListItem(
    val id: String,
    @SerialName("tmdb_id") val tmdbId: Long? = null,
    @SerialName("display_title") val displayTitle: String,
    val kind: String,
    @SerialName("is_anime") val isAnime: Boolean = false,
    @SerialName("torrent_count") val torrentCount: Long = 0,
    @SerialName("total_size_bytes") val totalSizeBytes: Long = 0,
    @SerialName("episode_count") val episodeCount: Long = 0,
    @SerialName("representative_infohash") val representativeInfohash: String? = null,
)

fun tmdbPosterUrl(path: String?, size: String = "w342"): String? =
    path?.let { "https://image.tmdb.org/t/p/$size$it" }

/** Backdrop sizes: w300, w780, w1280, original. We use w780 for shelf cards. */
fun tmdbBackdropUrl(path: String?, size: String = "w780"): String? =
    path?.let { "https://image.tmdb.org/t/p/$size$it" }
