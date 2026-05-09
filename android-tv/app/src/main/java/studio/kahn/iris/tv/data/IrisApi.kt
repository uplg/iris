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

    /** Add a torrent to Iris from a search hit. Returns the snapshot + id. */
    @POST("api/torrents")
    suspend fun ingest(@Body body: IngestRequest): IngestResponse

    @GET("api/me/devices")
    suspend fun listDevices(): List<DeviceView>

    @retrofit2.http.DELETE("api/me/devices/{jti}")
    suspend fun revokeDevice(@Path("jti") jti: String)
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
 * the file isn't yet ready, and `null` once `ready == true`. `progress` is
 * a 0..1 download fraction, only meaningful when `reason == "downloading"`.
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

fun tmdbPosterUrl(path: String?, size: String = "w342"): String? =
    path?.let { "https://image.tmdb.org/t/p/$size$it" }

/** Backdrop sizes: w300, w780, w1280, original. We use w780 for shelf cards. */
fun tmdbBackdropUrl(path: String?, size: String = "w780"): String? =
    path?.let { "https://image.tmdb.org/t/p/$size$it" }
