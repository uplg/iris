package studio.kahn.iris.tv.data

import okhttp3.Cookie
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SessionCookieJarTest {
    private val server = "https://iris.example.net"
    private val now = 1_800_000_000_000L
    private val stored = listOf(
        "iris_access=a1; path=/; secure; httponly",
        "iris_refresh=r1; expires=Sat, 01 Jan 2028 00:00:00 GMT; path=/api/auth; secure; httponly",
    )

    @Test
    fun theOriginIsSchemeHostAndPort() {
        assertTrue(isServerOrigin(server, "https://iris.example.net/api/me".toHttpUrl()))
        assertTrue(isServerOrigin(server, "https://IRIS.example.net:443/x".toHttpUrl()))
        assertFalse(isServerOrigin(server, "https://synthe.se/app-release.apk".toHttpUrl()))
        assertFalse(isServerOrigin(server, "https://uplg.xyz/app-release.version".toHttpUrl()))
        assertFalse(isServerOrigin(server, "http://iris.example.net/api/me".toHttpUrl()))
        assertFalse(isServerOrigin(server, "https://iris.example.net:8443/api/me".toHttpUrl()))
        assertFalse(isServerOrigin(server, "https://evil.iris.example.net/api/me".toHttpUrl()))
        assertTrue(isServerOrigin("http://192.168.1.10:8080", "http://192.168.1.10:8080/api/me".toHttpUrl()))
        assertFalse(isServerOrigin("http://192.168.1.10:8080", "http://192.168.1.10/api/me".toHttpUrl()))
    }

    @Test
    fun noCookieLeavesForAnotherHost() {
        assertEquals(emptyList<Cookie>(), sessionCookiesFor(server, stored, "https://synthe.se/app-release.apk".toHttpUrl(), now))
        assertEquals(emptyList<Cookie>(), sessionCookiesFor(server, stored, "http://iris.example.net/api/me".toHttpUrl(), now))
    }

    @Test
    fun cookiesFollowTheirPath() {
        val api = sessionCookiesFor(server, stored, "https://iris.example.net/api/me".toHttpUrl(), now).map { it.name }
        assertEquals(listOf("iris_access"), api)
        val refresh = sessionCookiesFor(server, stored, "https://iris.example.net/api/auth/refresh".toHttpUrl(), now).map { it.name }
        assertEquals(listOf("iris_access", "iris_refresh"), refresh)
    }

    @Test
    fun expiredCookiesAreNotSent() {
        val later = 1_900_000_000_000L
        val sent = sessionCookiesFor(server, stored, "https://iris.example.net/api/auth/refresh".toHttpUrl(), later).map { it.name }
        assertEquals(listOf("iris_access"), sent)
    }

    @Test
    fun onlyTheServerSetsCookies() {
        val foreign = "https://synthe.se/app-release.version".toHttpUrl()
        val cookie = Cookie.parse(foreign, "tracker=1; path=/")!!
        assertNull(mergeSessionCookies(server, stored, foreign, listOf(cookie), now))
    }

    @Test
    fun aRotationReplacesByName() {
        val url = "https://iris.example.net/api/auth/refresh".toHttpUrl()
        val rotated = Cookie.parse(url, "iris_access=a2; path=/; secure; httponly")!!
        val merged = mergeSessionCookies(server, stored, url, listOf(rotated), now)!!
        val sent = sessionCookiesFor(server, merged, url, now).associate { it.name to it.value }
        assertEquals(mapOf("iris_refresh" to "r1", "iris_access" to "a2"), sent)
    }

    @Test
    fun aLogoutCookieDropsTheStoredOne() {
        val url = "https://iris.example.net/api/auth/logout".toHttpUrl()
        val cleared = Cookie.parse(url, "iris_refresh=; max-age=0; path=/api/auth")!!
        val merged = mergeSessionCookies(server, stored, url, listOf(cleared), now)!!
        assertEquals(1, merged.size)
        assertTrue(merged.single().startsWith("iris_access=a1"))
    }
}
