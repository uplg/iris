package studio.kahn.iris.tv.data

import java.util.UUID
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.SerializationException
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test
import retrofit2.HttpException
import studio.kahn.iris.tv.ui.screens.player.LanguageChoices
import studio.kahn.iris.tv.ui.screens.search.TITLES_ABSENT
import studio.kahn.iris.tv.ui.screens.search.titlesSaid
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UNREADABLE_MESSAGE
import studio.kahn.iris.tv.ui.state.load
import studio.kahn.iris.tv.ui.state.toUiError

/** The app updated before the server: what the server does not know yet degrades, said in words. */
class OlderServerTest {
    private fun response(path: String, code: Int, type: String, body: String = "<!doctype html>") = Response.Builder()
        .request(Request.Builder().url("https://iris.example$path").build())
        .protocol(Protocol.HTTP_1_1)
        .code(code)
        .message("OK")
        .body(body.toResponseBody(type.toMediaType()))
        .build()

    @Test
    fun theWebAppAnsweringAnApiPathIsANotFound() {
        assertEquals(404, spaFallbackAsNotFound(response("/api/me/summary", 200, "text/html; charset=utf-8")).code)
        val json = response("/api/me", 200, "application/json", "{}")
        assertSame(json, spaFallbackAsNotFound(json))
        val page = response("/", 200, "text/html")
        assertSame(page, spaFallbackAsNotFound(page))
    }

    @Test
    fun anAbsentEndpointReadsAsItsFallback() = runTest {
        assertNull(absentAs(null) { throw HttpException(retrofit2.Response.error<Unit>(404, "".toResponseBody())) })
        val failed = load(Loadable.Loading) {
            absentAs(emptyList<String>()) { throw HttpException(retrofit2.Response.error<Unit>(500, "".toResponseBody())) }
        }
        assertEquals(500, failed.errorOrNull?.status)
    }

    @Test
    fun olderServersSayWhyInWords() = runTest {
        val notAllowed = HttpException(retrofit2.Response.error<Unit>(405, "".toResponseBody())).toUiError()
        assertEquals("The Iris server is older than this app and can't do this yet. Update the server.", notAllowed.message)
        assertEquals(UNREADABLE_MESSAGE, SerializationException("Unexpected JSON token at offset 0: <!doctype").toUiError().message)
        val titles = titlesSaid(load(Loadable.Loading) { throw HttpException(retrofit2.Response.error<Unit>(404, "".toResponseBody())) })
        assertEquals(TITLES_ABSENT, titles.errorOrNull?.message)
    }

    @Test
    fun aPickUnderATitleOnAServerWithoutPerTitleChoicesSendsTheWholeAccountState() {
        // No `for_collection`: the server would overwrite both account fields with what is sent.
        val old = PlaybackPrefsResponse(audioLanguage = "kor", subtitleLanguage = "fre")
        val picked = LanguageChoices.of(old, UUID.randomUUID()).audioPicked("eng")
        assertEquals(UpdatePlaybackPrefs(audioLanguage = "eng", subtitleLanguage = "fre", collectionId = null), picked.body())
    }
}
