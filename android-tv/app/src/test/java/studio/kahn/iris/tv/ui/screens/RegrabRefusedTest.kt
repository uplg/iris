package studio.kahn.iris.tv.ui.screens

import androidx.lifecycle.SavedStateHandle
import androidx.test.core.app.ApplicationProvider
import java.time.OffsetDateTime
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import kotlinx.coroutines.withTimeout
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.DefaultAppContainer
import studio.kahn.iris.tv.data.HistoryItem
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.ui.screens.library.HistoryViewModel
import studio.kahn.iris.tv.ui.screens.search.SearchViewModel
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.SearchInstead
import studio.kahn.iris.tv.ui.state.fakeApi
import studio.kahn.iris.tv.ui.state.http

/** "Download again" refused by the server lands on Search, the title asked, the refusal said. */
@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
class RegrabRefusedTest {
    private class Faked(real: AppContainer, private val api: IrisApi) : AppContainer by real {
        override fun apiFor(baseUrl: String): IrisApi = api
    }

    private val gone = HistoryItem(
        completed = false, deleted = true, fileIdx = 0, infohash = "ee01", lastWatchedAt = OffsetDateTime.now().minusDays(3),
        positionSeconds = 3_100.0, tmdbVerified = true, torrentName = "Past.Lives.2023.1080p.WEB", collectionTitle = "Past Lives",
        durationSeconds = 6_300.0, sourceProvider = "tr4ker", sourceExternalId = "z1", kind = MediaKind.movie,
    )
    private val calls = mutableListOf<Pair<String, List<Any?>>>()
    private val api = fakeApi(
        "history" to { listOf(gone) },
        "regrabTorrent" to { throw http(409, "provider_off", "This tracker is turned off in Admin.") },
        calls = calls,
    )

    private fun container(): AppContainer {
        val c = Faked(DefaultAppContainer(ApplicationProvider.getApplicationContext()), api)
        runBlocking { c.sessionStore.setServerUrl("https://iris.example/") }
        return c
    }

    @Before
    fun main() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @After
    fun reset() = Dispatchers.resetMain()

    @Test
    fun aRefusedRestoreFromHistorySearchesTheTitleInstead() = runBlocking {
        val container = container()
        val history = HistoryViewModel(container)
        val polling = launch(Dispatchers.Default) { history.pollWhileStarted() }
        val line = withTimeout(5_000) {
            history.state.first { it.groups is Loadable.Ready && it.groups.valueOrNull!!.isNotEmpty() }
        }.groups.valueOrNull!!.single().lines.single()
        polling.cancelAndJoin()

        history.restore(line)
        val instead = withTimeout(5_000) { history.searchInstead.pending.filterNotNull().first() }
        assertEquals(
            SearchInstead("Past Lives", "This tracker is turned off in Admin. Here are other releases of Past Lives."),
            instead,
        )

        val search = SearchViewModel(container, instead.query, autoPlay = false, SavedStateHandle(), instead.notice)
        withTimeout(5_000) { search.state.first { it.query == "Past Lives" } }
        withTimeout(5_000) { while (calls.none { it.first == "search" }) kotlinx.coroutines.delay(10) }
        assertEquals("Past Lives", calls.first { it.first == "search" }.second.first())
        assertEquals(instead.notice, search.state.value.notice)
        assertEquals("Past Lives", search.state.value.typed)

        search.submit("Columbus")
        assertEquals(null, search.state.value.notice)
    }
}
