package studio.kahn.iris.tv.data

import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import okhttp3.Authenticator
import okhttp3.ConnectionPool
import okhttp3.Dispatcher
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.Route
import okhttp3.logging.HttpLoggingInterceptor
import studio.kahn.iris.tv.BuildConfig
import java.util.concurrent.TimeUnit

/** Standard `X-Iris-Client: <kind>/<semver>` header. Parsed server-side
 *  by `client_version::client_version_layer` for telemetry + the
 *  `426 Upgrade Required` gate. */
const val IRIS_CLIENT_HEADER = "X-Iris-Client"

/** HTTP 426 — server tells us we're below `MIN_TV_VERSION`. */
private const val HTTP_UPGRADE_REQUIRED = 426

/**
 * Shared OkHttp client used by both Retrofit (API calls) and Media3
 * (segment fetches). Single client = single cookie jar = single session.
 *
 * The bundled [IrisAuthenticator] handles 401s transparently by hitting
 * `/api/auth/refresh` with the persisted refresh cookie and replaying the
 * original request. If refresh fails the 401 propagates up.
 *
 * [onOutdated] fires once the server answers `426 Upgrade Required` on
 * any request — the container flips its `clientOutdated` flow so the
 * root composable can swap in a "please update" lock-out screen.
 */
fun buildOkHttpClient(
    sessionStore: SessionStore,
    onOutdated: () -> Unit,
): OkHttpClient {
    val authenticator = IrisAuthenticator(sessionStore)
    // Cache the Iris-Caps header value once. Build.VERSION fields don't
    // change at runtime, so re-computing per request would be wasted work.
    val capsHeaderValue = IrisCaps.headerValue()
    val clientHeaderValue = "tv/${BuildConfig.VERSION_NAME}"
    val client = OkHttpClient.Builder()
        .cookieJar(SessionCookieJar(sessionStore))
        .authenticator(authenticator)
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .callTimeout(120, TimeUnit.SECONDS)
        // Stamp Iris-Caps + X-Iris-Client on every outbound request. The
        // server middleware ignores Iris-Caps on non-/torrents paths and
        // uses X-Iris-Client globally (telemetry + version gate). The
        // per-request cost is two ~200-byte header lines, much cheaper
        // than maintaining per-Retrofit-method annotation lists; Media3
        // segment fetches inherit both without extra plumbing.
        .addInterceptor { chain ->
            val request = chain.request().newBuilder()
                .header(IRIS_CAPS_HEADER, capsHeaderValue)
                .header(IRIS_CLIENT_HEADER, clientHeaderValue)
                .build()
            val response = chain.proceed(request)
            if (response.code == HTTP_UPGRADE_REQUIRED) {
                onOutdated()
            }
            response
        }
        .addInterceptor(
            HttpLoggingInterceptor().apply {
                level = HttpLoggingInterceptor.Level.BASIC
            }
        )
        .build()
    authenticator.bind(client)
    return client
}

/**
 * Media3-specific OkHttp client, forked off the API client.
 *
 * Why a separate client: Fire TV (Fire OS, AOSP-derivative) has been
 * observed wedging its network layer after an ExoPlayer Range-request
 * cancel on a seek (rewind in particular drops the entire internal
 * buffer, which forces a brand-new Range request immediately on top of
 * the cancellation). Symptom: rewind once, every subsequent call —
 * Media3 AND API — hangs forever, "totalement frozen". Standard
 * Android TVs with a current stack handle the cancellation cleanly.
 *
 * Two changes vs the API client:
 *
 * 1. **Dedicated `ConnectionPool` + `Dispatcher`.** Total isolation from
 *    the Retrofit client. If the Media3 pool DOES wedge on a Fire OS
 *    cancel race, it stays local to Media3 — API calls (back nav →
 *    other movie → manifest fetch) keep working on their own pool.
 *    The pool isolation alone is what lets the user recover by going
 *    back to the home screen and trying a different file instead of
 *    needing to kill the app.
 *
 * 2. **`callTimeout = 0` (disabled).** A streamed Range body
 *    legitimately spans many minutes while ExoPlayer drip-reads at
 *    playback rate; the API client's 120 s ceiling would hard-kill the
 *    call mid-buffer and ExoPlayer's retry would just rebuild the same
 *    dying request. `readTimeout` (per-read) still bounds genuine
 *    stalls.
 *
 * Stays on HTTP/2 like the API client — lower handshake cost on seeks
 * keeps rewind snappy on healthy stacks. If a Fire TV still wedges
 * inside Media3 after this change, the escalation is to add
 * `.protocols(listOf(Protocol.HTTP_1_1))` here — that gives each Range
 * request its own TCP socket so cancel = `close(socket)` net, at the
 * cost of one extra RTT per seek on every device.
 */
fun deriveMediaOkHttpClient(api: OkHttpClient): OkHttpClient =
    api.newBuilder()
        .connectionPool(ConnectionPool(5, 5, TimeUnit.MINUTES))
        .dispatcher(Dispatcher())
        .callTimeout(0, TimeUnit.MILLISECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()

/**
 * OkHttp [Authenticator] that transparently refreshes the access cookie when
 * the server replies 401. The refresh request flows through the same client
 * (so the cookie jar is updated atomically), but we short-circuit on auth
 * paths to avoid recursion if the refresh itself returns 401.
 *
 * Concurrent 401s on multiple in-flight requests funnel through [refreshing]
 * so we only fire one /auth/refresh per stampede.
 */
class IrisAuthenticator(private val sessionStore: SessionStore) : Authenticator {

    @Volatile private var refreshClient: OkHttpClient? = null
    private val lock = Any()
    // Bumped on every SUCCESSFUL refresh. A 401 thread reads it before taking
    // the lock; if it advanced while the thread waited, another thread already
    // refreshed → replay with the new cookie instead of burning a second
    // refresh (a redundant refresh rotates the token again and can 401 the
    // stragglers that are still mid-flight on the previous token).
    @Volatile private var refreshGeneration = 0

    /** Wired by [buildOkHttpClient] once the client itself exists. */
    fun bind(client: OkHttpClient) {
        refreshClient = client
    }

    override fun authenticate(route: Route?, response: Response): Request? {
        val path = response.request.url.encodedPath
        if (path.endsWith("/auth/refresh")
            || path.endsWith("/auth/login")
            || path.endsWith("/auth/logout")
            || path.contains("/auth/device/code")
            || path.contains("/auth/device/poll/")
        ) {
            return null
        }
        // OkHttp passes priorResponse on its own retry loop; if we already
        // tried once, give up to avoid infinite retries.
        if (response.priorResponse != null) return null

        val genAtEntry = refreshGeneration
        // Serialize concurrent 401s through one refresh. The OLD code used a
        // non-blocking flag and replayed the other threads IMMEDIATELY — they
        // re-sent the still-expired access token before the refresh landed,
        // 401'd again, and surfaced as a spurious error. Holding the lock means
        // stragglers wait for the in-flight refresh, then replay with the fresh
        // cookie.
        synchronized(lock) {
            // Someone refreshed while we waited → just replay, don't refresh
            // again.
            if (refreshGeneration != genAtEntry) {
                return response.request.newBuilder().build()
            }
            val client = refreshClient ?: return null
            val baseUrl = runBlocking { sessionStore.serverUrl.first() } ?: return null
            val refreshUrl = (if (baseUrl.endsWith("/")) baseUrl else "$baseUrl/") + "api/auth/refresh"
            val refreshReq = Request.Builder()
                .url(refreshUrl)
                .post(ByteArray(0).toRequestBody())
                .build()
            val code = try {
                client.newCall(refreshReq).execute().use { it.code }
            } catch (_: Exception) {
                // Network / IO error — NOT an auth failure. Leave the session
                // intact so a retry (or the next launch) recovers; nuking it
                // here would log the TV out on every Wi-Fi blip.
                return null
            }
            return when {
                code in 200..299 -> {
                    // CookieJar.saveFromResponse already persisted the rotated
                    // cookies.
                    refreshGeneration++
                    response.request.newBuilder().build()
                }
                code == 401 || code == 403 -> {
                    // The refresh token itself is dead (expired / revoked /
                    // server secret rotated). Replaying it forever is exactly
                    // the "401 + Retry that never reconnects" trap. Drop the
                    // session so the nav root routes the TV back to device
                    // pairing instead of stranding the user.
                    runBlocking { runCatching { sessionStore.clear() } }
                    null
                }
                // 5xx / other: transient server-side, keep the session so the
                // user can retry once the backend is back.
                else -> null
            }
        }
    }
}
