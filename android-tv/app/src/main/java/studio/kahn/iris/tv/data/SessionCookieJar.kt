package studio.kahn.iris.tv.data

import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import okhttp3.Cookie
import okhttp3.CookieJar
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull

/**
 * OkHttp [CookieJar] backed by [SessionStore]. Persists the cookies set by
 * `/auth/login` so subsequent app launches replay the same session — we
 * never need to ask for the password again until the refresh token expires.
 *
 * The session's cookies belong to the paired server's origin only (scheme,
 * host, port of `serverUrl`): they are never sent to, nor taken from, any
 * other host (the APK and its version sidecar live on another one).
 *
 * `runBlocking` is intentional and safe here: the OkHttp interceptor chain
 * runs on its own thread (never on Compose / main), and the DataStore flow
 * resolves to the cached value almost instantly.
 */
class SessionCookieJar(private val store: SessionStore) : CookieJar {

    override fun loadForRequest(url: HttpUrl): List<Cookie> {
        val session = runBlocking { store.session.first() } ?: return emptyList()
        return sessionCookiesFor(session.serverUrl, session.cookies, url, System.currentTimeMillis())
    }

    override fun saveFromResponse(url: HttpUrl, cookies: List<Cookie>) {
        if (cookies.isEmpty()) return
        runBlocking {
            val current = store.session.first() ?: return@runBlocking
            val merged = mergeSessionCookies(current.serverUrl, current.cookies, url, cookies, System.currentTimeMillis()) ?: return@runBlocking
            store.saveSession(current.copy(cookies = merged))
        }
    }
}

/** True when [url] is on the origin of [serverUrl]: same scheme, host and port. */
fun isServerOrigin(serverUrl: String, url: HttpUrl): Boolean {
    val server = serverUrl.toHttpUrlOrNull() ?: return false
    return server.scheme == url.scheme && server.host.equals(url.host, ignoreCase = true) && server.port == url.port
}

/**
 * The stored cookies to send with a request to [url]: none off the server's origin, else
 * those that match it (path, Secure) and have not expired. Stored cookies are parsed against
 * the server's URL, so a host-only one stays on the server's host.
 */
fun sessionCookiesFor(serverUrl: String, stored: List<String>, url: HttpUrl, nowMs: Long): List<Cookie> {
    if (!isServerOrigin(serverUrl, url)) return emptyList()
    val server = serverUrl.toHttpUrlOrNull() ?: return emptyList()
    return stored.mapNotNull { Cookie.parse(server, it) }
        .filter { it.expiresAt > nowMs && it.matches(url) }
}

/**
 * The session's cookies once [incoming] from a response to [url] are kept: each replaces the
 * stored one of the same name, an expired one (a logout) drops it; null when [url] is off the
 * server's origin (nothing to keep).
 */
fun mergeSessionCookies(serverUrl: String, stored: List<String>, url: HttpUrl, incoming: List<Cookie>, nowMs: Long): List<String>? {
    if (!isServerOrigin(serverUrl, url)) return null
    val server = serverUrl.toHttpUrlOrNull() ?: return null
    val names = incoming.mapTo(HashSet()) { it.name }
    val kept = stored.mapNotNull { Cookie.parse(server, it) }.filter { it.name !in names }
    return (kept + incoming).filter { it.expiresAt > nowMs }.map { it.toString() }
}
