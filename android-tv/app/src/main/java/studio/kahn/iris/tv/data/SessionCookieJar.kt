package studio.kahn.iris.tv.data

import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import okhttp3.Cookie
import okhttp3.CookieJar
import okhttp3.HttpUrl

/**
 * OkHttp [CookieJar] backed by [SessionStore]. Persists the cookies set by
 * `/auth/login` so subsequent app launches replay the same session — we
 * never need to ask for the password again until the refresh token expires.
 *
 * `runBlocking` is intentional and safe here: the OkHttp interceptor chain
 * runs on its own thread (never on Compose / main), and the DataStore flow
 * resolves to the cached value almost instantly.
 */
class SessionCookieJar(private val store: SessionStore) : CookieJar {

    override fun loadForRequest(url: HttpUrl): List<Cookie> {
        val session = runBlocking { store.session.first() } ?: return emptyList()
        return session.cookies.mapNotNull { Cookie.parse(url, it) }
    }

    override fun saveFromResponse(url: HttpUrl, cookies: List<Cookie>) {
        if (cookies.isEmpty()) return
        runBlocking {
            val current = store.session.first() ?: return@runBlocking
            // Replace any cookie whose name we saw in the response; preserve
            // the rest. Cookies are persisted as their original Set-Cookie
            // string so attributes (HttpOnly, Path, Secure) survive.
            val incomingNames = cookies.map { it.name }.toSet()
            val merged = current.cookies
                .mapNotNull { Cookie.parse(url, it) }
                .filter { it.name !in incomingNames } + cookies
            store.saveSession(
                current.copy(cookies = merged.map { it.toString() })
            )
        }
    }
}
