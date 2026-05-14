package studio.kahn.iris.tv.data

import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import okhttp3.Authenticator
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.Route
import okhttp3.logging.HttpLoggingInterceptor
import studio.kahn.iris.tv.BuildConfig
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

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
    private val refreshing = AtomicBoolean(false)

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

        if (!refreshing.compareAndSet(false, true)) {
            // Another thread is already refreshing — let it finish, then just
            // replay our original request which will pick up the new cookie.
            return response.request.newBuilder().build()
        }
        try {
            val baseUrl = runBlocking { sessionStore.serverUrl.first() } ?: return null
            val client = refreshClient ?: return null
            val refreshUrl = (if (baseUrl.endsWith("/")) baseUrl else "$baseUrl/") + "api/auth/refresh"
            val refreshReq = Request.Builder()
                .url(refreshUrl)
                .post(ByteArray(0).toRequestBody())
                .build()
            val refreshRes = client.newCall(refreshReq).execute()
            refreshRes.use { res ->
                if (!res.isSuccessful) return null
            }
            // CookieJar.saveFromResponse already persisted the new cookies.
            return response.request.newBuilder().build()
        } catch (_: Exception) {
            return null
        } finally {
            refreshing.set(false)
        }
    }
}
