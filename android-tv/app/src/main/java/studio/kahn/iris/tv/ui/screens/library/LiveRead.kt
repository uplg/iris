package studio.kahn.iris.tv.ui.screens.library

import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.load

/** Live progress: quick while something moves, slow otherwise (web `queries.ts`). */
const val FAST_MS = 3_000L
const val SLOW_MS = 30_000L

/**
 * A server read a ViewModel keeps, refreshed at an interval that depends on what it last
 * read (quick while a download moves). Unlike `pollWhileStarted`, an action can [refresh] it
 * and wait for the server's answer before anything is shown as changed (no optimistic UI).
 * [poll] runs only while the screen is started (call it from `RepeatWhileStarted`).
 * Equal reads do not emit, so a quiet poll recomposes nothing.
 */
class LiveRead<T>(
    private val intervalMs: (T?) -> Long = { SLOW_MS },
    private val fetch: suspend () -> T,
) {
    private val mutable = MutableStateFlow<Loadable<T>>(Loadable.Loading)
    val state: StateFlow<Loadable<T>> = mutable.asStateFlow()
    private val wake = Channel<Unit>(Channel.CONFLATED)
    private val lock = Mutex()

    val value: T? get() = mutable.value.valueOrNull

    suspend fun refresh() {
        lock.withLock { mutable.value = load(mutable.value, fetch) }
    }

    /** Reads again now, then keeps polling. */
    fun poke() {
        wake.trySend(Unit)
    }

    suspend fun poll() {
        while (true) {
            refresh()
            withTimeoutOrNull(intervalMs(value)) { wake.receive() }
        }
    }
}

/** The API of the paired server; fails (as a read error) when the TV is signed out. */
suspend fun AppContainer.api(): IrisApi {
    val url = sessionStore.serverUrl.first() ?: throw IllegalStateException("This TV is signed out. Pair it again from Settings.")
    return apiFor(url)
}
