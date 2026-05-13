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
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Shared OkHttp client used by both Retrofit (API calls) and Media3
 * (segment fetches). Single client = single cookie jar = single session.
 *
 * The bundled [IrisAuthenticator] handles 401s transparently by hitting
 * `/api/auth/refresh` with the persisted refresh cookie and replaying the
 * original request. If refresh fails the 401 propagates up.
 */
fun buildOkHttpClient(sessionStore: SessionStore): OkHttpClient {
    val authenticator = IrisAuthenticator(sessionStore)
    // Cache the Iris-Caps header value once. Build.VERSION fields don't
    // change at runtime, so re-computing per request would be wasted work.
    val capsHeaderValue = IrisCaps.headerValue()
    val client = OkHttpClient.Builder()
        .cookieJar(SessionCookieJar(sessionStore))
        .authenticator(authenticator)
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .callTimeout(120, TimeUnit.SECONDS)
        // Stamp Iris-Caps on every outbound request. The server middleware
        // ignores it on non-/torrents paths; the per-request cost is one
        // ~200-byte header line. Cheaper than maintaining a per-Retrofit-
        // method header annotation list, and Media3 segment fetches inherit
        // the header without extra plumbing.
        .addInterceptor { chain ->
            val request = chain.request().newBuilder()
                .header(IRIS_CAPS_HEADER, capsHeaderValue)
                .build()
            chain.proceed(request)
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
