package studio.kahn.iris.tv.data

import retrofit2.http.Body
import retrofit2.http.DELETE
import retrofit2.http.GET
import retrofit2.http.POST
import retrofit2.http.PUT
import retrofit2.http.Path
import retrofit2.http.Query

/**
 * Iris HTTP API surface. Mirrors what the web client uses; we deliberately
 * skip the heavier endpoints (admin, ingest, etc.) until the corresponding
 * TV screens land.
 *
 * The request/response DTOs are NOT defined here — they are generated from the
 * backend's committed OpenAPI spec (`../web/openapi.json`) by the
 * `openApiGenerate` task into this same `…data` package, so the contract can't
 * drift from the server. This interface (the endpoint surface + auth/cookie/
 * caps wiring around it) stays hand-written. See `app/build.gradle.kts`.
 */
interface IrisApi {

    @POST("api/auth/login")
    suspend fun login(@Body body: LoginRequest): UserResponse

    @POST("api/auth/device/code")
    suspend fun createDeviceCode(@Body body: CreateCodeRequest): CreateCodeResponse

    @GET("api/auth/device/poll/{deviceId}")
    suspend fun pollDeviceCode(@Path("deviceId") deviceId: String): PollResponse

    @POST("api/auth/refresh")
    suspend fun refresh(): UserResponse

    @POST("api/auth/logout")
    suspend fun logout()

    @GET("api/me")
    suspend fun me(): UserResponse

    @GET("api/me/continue-watching")
    suspend fun continueWatching(): List<ContinueWatchingItem>

    /** Full watch history — in-progress AND completed, including items whose
     *  source torrent has since been deleted (see [HistoryItem.deleted]). */
    @GET("api/me/history")
    suspend fun history(
        @Query("limit") limit: Int? = null,
        @Query("offset") offset: Int? = null,
    ): List<HistoryItem>

    /** Per-user recommendation preferences (languages / genres / anime).
     *  A never-onboarded user gets the all-empty default with
     *  `onboarding_completed = false`. */
    @GET("api/me/preferences")
    suspend fun preferences(): PreferencesResponse

    @PUT("api/me/preferences")
    suspend fun savePreferences(@Body body: UpdatePreferencesRequest): PreferencesResponse

    /** Merged movie + TV genre taxonomy for the onboarding picker. Anime
     *  is NOT in here — it's a distinct category driven by
     *  [UpdatePreferencesRequest.includeAnime]. */
    @GET("api/genres")
    suspend fun genres(): GenresResponse

    /** Server-driven selectable languages for onboarding — fetched so a
     *  new language never requires an APK release. */
    @GET("api/languages")
    suspend fun languages(): LanguagesResponse

    /** The home blended "For You" shelf. */
    @GET("api/me/for-you")
    suspend fun forYou(): ForYou

    /** The organized "For You" page (top picks + per-genre + anime). */
    @GET("api/me/for-you/page")
    suspend fun forYouPage(): ForYou

    /** Hide a recommendation candidate from future shelves. */
    @POST("api/me/for-you/dismiss")
    suspend fun dismissForYou(@Body body: DismissRequest)

    /** The mood board for a kind — TMDB genres ordered by the user's taste. */
    @GET("api/me/moods")
    suspend fun moodBoard(@Query("kind") kind: String): MoodBoard

    /** Results for a mood — catalogue ∪ broad TMDB, recency-filtered, taste-ranked. */
    @GET("api/me/moods/{id}")
    suspend fun moodResults(@Path("id") id: String, @Query("kind") kind: String): MoodResults

    /** The user's preferred audio + subtitle language (applied across episodes
     *  / devices). */
    @GET("api/me/playback-preferences")
    suspend fun playbackPreferences(): PlaybackPrefsResponse

    /** Save preferred audio + subtitle language. Send the full current state. */
    @PUT("api/me/playback-preferences")
    suspend fun savePlaybackPreferences(@Body body: UpdatePlaybackPrefs)

    @GET("api/torrents")
    suspend fun listTorrents(): List<TorrentView>

    @GET("api/torrents/{infohash}")
    suspend fun getTorrent(@Path("infohash") infohash: String): TorrentView

    /** Remove a torrent: the backend also wipes its files from disk and
     *  soft-deletes the row (204 No Content). Any authenticated user —
     *  the seedbox view is single-household, same as the web client. */
    @DELETE("api/torrents/{infohash}")
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

    @PUT("api/torrents/{infohash}/files/{idx}/progress")
    suspend fun saveProgress(
        @Path("infohash") infohash: String,
        @Path("idx") idx: Int,
        @Body body: ProgressUpdate,
    )

    /** Remove a file from the caller's Continue Watching + history. */
    @DELETE("api/torrents/{infohash}/files/{idx}/progress")
    suspend fun removeProgress(
        @Path("infohash") infohash: String,
        @Path("idx") idx: Int,
    )

    /** Mark a file watched for the caller (also skips a "next up" tile). */
    @POST("api/torrents/{infohash}/files/{idx}/progress/complete")
    suspend fun markWatched(
        @Path("infohash") infohash: String,
        @Path("idx") idx: Int,
    )

    /** Remove a tile from Continue Watching — a whole TV series (pass its
     *  `collectionId`) or a movie / standalone (pass `infohash` + `fileIdx`). */
    @POST("api/me/continue-watching/dismiss")
    suspend fun dismissContinueWatching(@Body body: DismissCwRequest)

    /** Remove from MY Watchlist (auto-recreated on next grab/play). */
    @POST("api/me/watchlist/remove")
    suspend fun removeFromWatchlist(@Body body: RemoveWatchlistRequest)

    /** Per-user hide of a Gone entry (ghost collection or single
     *  release). Newer activity resurfaces it. */
    @POST("api/me/gone/dismiss")
    suspend fun dismissGone(@Body body: DismissGoneRequest)

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
    ): MediaMetadata

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
    ): SearchResponse

    @GET("api/search/details")
    suspend fun torrentDetails(
        @Query("provider") provider: String,
        @Query("id") id: String,
    ): TorrentDetails

    /** Add a torrent to Iris from a search hit. Returns the snapshot + id. */
    @POST("api/torrents")
    suspend fun ingest(@Body body: ResolveBody): IngestResponse

    @GET("api/me/devices")
    suspend fun listDevices(): List<DeviceView>

    @DELETE("api/me/devices/{jti}")
    suspend fun revokeDevice(@Path("jti") jti: String)

    // ----------- Discovery + series follows (Phase 2 / Phase 4) -----------

    @GET("api/discover/featured")
    suspend fun discoverFeatured(): FeaturedResponse

    @GET("api/me/follows")
    suspend fun listFollows(): List<FollowSummary>

    @POST("api/me/follows")
    suspend fun addFollow(@Body body: CreateFollowRequest): FollowSummary

    @DELETE("api/me/follows/{id}")
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
    ): GrabResponse

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
    ): GrabResponse

    // ------------------------------ Live TV ------------------------------
    // Channels play via `api/livetv/{country}/channels/{id}/master.m3u8`
    // (built as a URL for Media3, not a Retrofit call) — the backend
    // rewrites every HLS URI to its signed proxy, so ExoPlayer only ever
    // talks to Iris.

    @GET("api/livetv/countries")
    suspend fun liveTvCountries(): LiveCountriesResponse

    @GET("api/livetv/{country}/channels")
    suspend fun liveTvChannels(@Path("country") country: String): LiveChannelsResponse

    /** Cross-country channel search (server-side, diacritics-insensitive). */
    @GET("api/livetv/search")
    suspend fun liveTvSearch(@Query("q") q: String): LiveSearchResponse

    /** Now/next programme per channel; empty when the country has no
     *  configured XMLTV guide. */
    @GET("api/livetv/{country}/epg/now")
    suspend fun liveTvEpgNow(@Path("country") country: String): LiveEpgNowResponse

    /** The served stream is unplayable client-side: the backend cools the
     *  active source down and elects the channel's next feed. */
    @POST("api/livetv/{country}/channels/{channelId}/playback-error")
    suspend fun liveTvPlaybackError(
        @Path("country") country: String,
        @Path("channelId") channelId: String,
    )
}
