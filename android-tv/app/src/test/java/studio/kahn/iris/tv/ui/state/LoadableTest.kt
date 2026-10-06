package studio.kahn.iris.tv.ui.state

import java.io.IOException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runTest
import okhttp3.ResponseBody.Companion.toResponseBody
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import retrofit2.HttpException
import retrofit2.Response
import studio.kahn.iris.tv.ui.screens.nextBackoff
import studio.kahn.iris.tv.ui.theme.columnsFor

class LoadableTest {
    private val boom = UiError("boom")

    @Test
    fun aFailedRefreshKeepsTheValueAsStale() {
        assertEquals(Loadable.Stale(1, boom), Loadable.Ready(1).failedWith(boom))
        assertEquals(Loadable.Stale(1, boom), Loadable.Stale(1, UiError("old")).failedWith(boom))
        assertEquals(Loadable.Failed(boom), Loadable.Loading.failedWith(boom))
        assertEquals(Loadable.Failed(boom), Loadable.Failed(UiError("old")).failedWith(boom))
    }

    @Test
    fun valueAndErrorAccessors() {
        assertEquals(2, Loadable.Stale(2, boom).valueOrNull)
        assertEquals(boom, Loadable.Stale(2, boom).errorOrNull)
        assertNull(Loadable.Ready(2).errorOrNull)
        assertNull(Loadable.Failed(boom).valueOrNull)
        assertEquals(Loadable.Stale("2", boom), Loadable.Stale(2, boom).map { it.toString() })
    }

    @Test
    fun loadMapsFailuresAndKeepsTheLastValue() = runTest {
        assertEquals(Loadable.Ready(3), load(Loadable.Loading) { 3 })
        val stale = load(Loadable.Ready(3)) { throw IOException("down") }
        assertEquals(Loadable.Stale(3, UiError(UiError.OFFLINE_MESSAGE, code = UiError.NETWORK)), stale)
    }

    @Test(expected = CancellationException::class)
    fun loadNeverSwallowsCancellation() = runTest {
        load(Loadable.Ready(3)) { throw CancellationException("gone") }
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun pollingGoesOnThroughFailures() = runTest {
        var call = 0
        val states = polling(intervalMs = 1_000, from = { Loadable.Loading }) {
            call++
            if (call == 2) throw IOException("blip") else call
        }.take(3).toList()
        assertEquals(Loadable.Ready(1), states[0])
        assertTrue(states[1] is Loadable.Stale && states[1].valueOrNull == 1)
        assertEquals(Loadable.Ready(3), states[2])
        assertEquals(2_000, currentTime)
    }

    @Test
    fun httpErrorsUseTheServerMessageThenAFallback() {
        val withEnvelope = HttpException(
            Response.error<Unit>(409, """{"error":"dead_torrent","message":"Nobody seeds this."}""".toResponseBody()),
        ).toUiError()
        assertEquals(UiError("Nobody seeds this.", code = "dead_torrent", status = 409), withEnvelope)
        val bare = HttpException(Response.error<Unit>(503, "".toResponseBody())).toUiError()
        assertEquals("The Iris server had a problem. Try again in a moment.", bare.message)
        assertEquals(503, bare.status)
    }

    @Test
    fun pollBackoffDoublesUpToFifteenSeconds() {
        assertEquals(4_000, nextBackoff(2_000))
        assertEquals(15_000, nextBackoff(10_000))
    }

    @Test
    fun columnsFollowTheWidth() {
        // TV: 864 dp between the 48 dp margins, 104 dp cells, 16 dp gaps.
        assertEquals(7, columnsFor(864f, 104f, 16f))
        // Landscape phone, 800 dp wide: 720 dp between its 40 dp margins.
        assertEquals(6, columnsFor(720f, 104f, 16f))
        assertEquals(1, columnsFor(50f, 104f, 16f))
    }
}
