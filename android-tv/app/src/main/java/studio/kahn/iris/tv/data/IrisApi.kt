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

    @GET("api/torrents")
    suspend fun listTorrents(): List<TorrentView>

    @GET("api/torrents/{infohash}")
    suspend fun getTorrent(@Path("infohash") infohash: String): TorrentView

    @GET("api/torrents/{infohash}/progress")
    suspend fun torrentProgress(@Path("infohash") infohash: String): List<FileProgressEntry>

    @GET("api/torrents/{infohash}/files/{idx}/probe")
    suspend fun probe(
        @Path("infohash") infohash: String,
        @Path("idx") idx: Int,
    ): MediaProbe

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

    @GET("api/metadata/tmdb/{id}")
    suspend fun tmdbMetadata(@Path("id") id: Long): TmdbMetadata

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

@Serializable
data class ContinueWatchingItem(
    val infohash: String,
    @SerialName("torrent_name") val torrentName: String,
    @SerialName("tmdb_id") val tmdbId: Long? = null,
    @SerialName("tmdb_verified") val tmdbVerified: Boolean = false,
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
    @SerialName("subtitle_track_idx") val subtitleTrackIdx: Int? = null,
    val completed: Boolean = false,
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

@Serializable
data class AggregatedResults(
    val results: List<SearchResult>,
    val providers: List<ProviderResultMeta>,
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

@Serializable
data class TorrentDetails(
    @SerialName("provider_id") val providerId: String,
    @SerialName("external_id") val externalId: String,
    val title: String,
    val description: String? = null,
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
)

/**
 * Polymorphic response from `/api/library?view=...`. The backend
 * tags each variant with the `view` field so kotlinx.serialization
 * can route to the right shape. We only consume `Collections` from TV
 * (the raw torrent view stays on the web admin panel) but the type
 * still models both for forward-compat.
 */
@Serializable
sealed class LibraryResponse {
    @Serializable
    @SerialName("collections")
    data class Collections(val items: List<CollectionListItem> = emptyList()) : LibraryResponse()

    @Serializable
    @SerialName("torrents")
    data class Torrents(val items: List<TorrentView> = emptyList()) : LibraryResponse()
}

@Serializable
data class CollectionListItem(
    val id: String,
    @SerialName("tmdb_id") val tmdbId: Long? = null,
    @SerialName("display_title") val displayTitle: String,
    val kind: String,
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
