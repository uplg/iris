package studio.kahn.iris.tv.ui.state

import java.lang.reflect.Proxy
import kotlinx.coroutines.test.runTest
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.ResponseBody.Companion.toResponseBody
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import retrofit2.HttpException
import retrofit2.Response
import studio.kahn.iris.tv.data.IrisApi

/** A refused "Download again" becomes the search to run instead, in the web's words. */
class RegrabTest {
    private val episode = Reclaimed("ab12", "Severance", season = 2, episode = 4, name = "Severance.S02E04.1080p.WEB")

    private fun refusing(status: Int, code: String?, message: String): IrisApi = fakeApi(
        "regrabTorrent" to { throw http(status, code, message) },
    )

    private suspend fun refused(status: Int, code: String?, message: String, r: Reclaimed = episode): SearchInstead {
        val res = regrab(refusing(status, code, message), r)
        assertTrue("$res", res is Regrabbed.Refused)
        return (res as Regrabbed.Refused).search
    }

    @Test
    fun aTrackerTurnedOffSaysSoAndSearchesTheEpisode() = runTest {
        assertEquals(
            SearchInstead("Severance S02E04", "This tracker is turned off in Admin. Here are other releases of Severance S02E04."),
            refused(409, "provider_off", "This tracker is turned off in Admin."),
        )
    }

    @Test
    fun eachRefusalInItsWords() = runTest {
        val movie = Reclaimed("cd34", "Past Lives", name = "Past.Lives.2023.1080p.WEB")
        assertEquals(
            "Nobody shares this release any more. Here are other releases of Past Lives.",
            refused(409, "dead_torrent", "this release has no seeders and can't be downloaded", movie).notice,
        )
        assertEquals(
            "Its tracker no longer has this release. Here are other releases of Past Lives.",
            refused(404, "not_found", "not found", movie).notice,
        )
        assertEquals(
            "Its tracker no longer has this release. Here are other releases of Past Lives.",
            refused(400, "bad_request", "provider: torrent 42 not found", movie).notice,
        )
        assertEquals(
            "The release is a packed archive. Here are other releases of Past Lives.",
            refused(409, "archive_only", "The release is a packed archive.", movie).notice,
        )
    }

    @Test
    fun withoutATitleTheReleaseNameIsSearched() = runTest {
        assertEquals("Past Lives 2023", refused(404, null, "", Reclaimed("cd34", null, name = "Past.Lives.2023.1080p.WEB")).query)
    }

    @Test
    fun noRefusalIsThrown() = runTest {
        for (status in listOf(401, 426, 500, 502)) {
            val thrown = runCatching { regrab(refusing(status, null, "x"), episode) }.exceptionOrNull()
            assertEquals(status, (thrown as? HttpException)?.code())
        }
    }
}

internal fun http(status: Int, code: String?, message: String): HttpException {
    val body = if (code == null) "{\"message\":\"$message\"}" else "{\"error\":\"$code\",\"message\":\"$message\"}"
    return HttpException(Response.error<Unit>(status, body.toResponseBody("application/json".toMediaType())))
}

/**
 * An [IrisApi] answering [answers] by method name (a suspend method answers without
 * suspending); any other call fails as the server would. [calls] records each one.
 */
internal fun fakeApi(vararg answers: Pair<String, () -> Any?>, calls: MutableList<Pair<String, List<Any?>>> = mutableListOf()): IrisApi {
    val byName = answers.toMap()
    return Proxy.newProxyInstance(IrisApi::class.java.classLoader, arrayOf(IrisApi::class.java)) { proxy, method, args ->
        when (method.name) {
            "toString" -> "fakeApi"
            "hashCode" -> System.identityHashCode(proxy)
            "equals" -> proxy === args?.firstOrNull()
            else -> {
                val a = args.orEmpty()
                calls += method.name to a.dropLast(1)
                val answer = byName[method.name] ?: throw http(500, null, "not faked: ${method.name}")
                answer()
            }
        }
    } as IrisApi
}
