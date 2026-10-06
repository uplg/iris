package studio.kahn.iris.tv.ui.state

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.rememberUpdatedState
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.ReceiveChannel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull

/**
 * One read: [Loadable.Ready] on success, else [previous] marked failed
 * (a value already shown turns [Loadable.Stale], nothing turns
 * [Loadable.Failed]). Cancellation propagates.
 */
suspend fun <T> load(previous: Loadable<T>, fetch: suspend () -> T): Loadable<T> =
    try {
        Loadable.Ready(fetch())
    } catch (e: Exception) {
        previous.failedWith(e.toUiError())
    }

/** Live progress: quick while something moves, slow otherwise (web `queries.ts` `FAST`/`SLOW`). */
const val FAST_MS = 3_000L
const val SLOW_MS = 30_000L

/**
 * The one polling loop: [read] now, then again [intervalMs] after each read ended, while
 * [keepGoing]. The interval is asked after every read, so the pace can follow what was read
 * (quick while a download moves); [wake] cuts a wait short (read again now).
 */
suspend fun pollWhile(
    intervalMs: () -> Long,
    keepGoing: () -> Boolean = { true },
    wake: ReceiveChannel<Unit>? = null,
    read: suspend () -> Unit,
) {
    while (keepGoing()) {
        read()
        if (!keepGoing()) return
        val wait = intervalMs()
        if (wake == null) delay(wait) else withTimeoutOrNull(wait) { wake.receive() }
    }
}

/**
 * Waits for something the server will do (another TV signing in): [done] asked every
 * [intervalMs], the first time one interval from now, for at most [timeoutMs]. True once done.
 */
suspend fun pollUntil(intervalMs: Long, timeoutMs: Long, done: suspend () -> Boolean): Boolean {
    var answered = false
    var waited = 0L
    pollWhile({ intervalMs }, keepGoing = { !answered && waited < timeoutMs }) {
        delay(intervalMs)
        waited += intervalMs
        answered = done()
    }
    return answered
}

/**
 * A read repeated every [intervalMs] while collected, starting from [from]
 * (the state already shown). A failed refresh keeps the last value as
 * [Loadable.Stale] and the polling goes on. Usually reached through
 * [pollWhileStarted].
 */
fun <T> polling(
    intervalMs: Long,
    from: () -> Loadable<T>,
    fetch: suspend () -> T,
): Flow<Loadable<T>> = polling({ _: T? -> intervalMs }, from, fetch)

/** [polling] at a pace that follows what was read last (slower once nothing moves). */
fun <T> polling(
    intervalMs: (T?) -> Long,
    from: () -> Loadable<T>,
    fetch: suspend () -> T,
): Flow<Loadable<T>> = flow {
    var state = from()
    pollWhile({ intervalMs(state.valueOrNull) }) {
        state = load(state, fetch)
        emit(state)
    }
}

/**
 * A server read a ViewModel keeps, refreshed at an interval that depends on what it last
 * read ([intervalMs], quick while a download moves). An action can [refresh] it and wait for
 * the server's answer before anything is shown as changed (no optimistic UI), or [poke] it
 * (read again now). [poll] runs the polling: call it only while the screen is started
 * (`RepeatWhileStarted`). Equal reads do not emit, so a quiet poll recomposes nothing.
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

    suspend fun poll() = pollWhile({ intervalMs(value) }, wake = wake) { refresh() }
}

/** How long a flow keeps running after the last collector left (a rotation, a quick back-and-forth). */
const val STOP_TIMEOUT_MS = 5_000L

/**
 * A ViewModel's polled read: `val torrents = viewModelScope.pollWhileStarted(5_000) { api.torrents() }`.
 * Collected with `collectAsStateWithLifecycle()`, it polls only while the
 * screen is started (it stops [STOP_TIMEOUT_MS] after Home, the screensaver
 * or another screen on top) and resumes from the value it last showed.
 */
fun <T> CoroutineScope.pollWhileStarted(intervalMs: Long, fetch: suspend () -> T): StateFlow<Loadable<T>> =
    pollWhileStarted({ _: T? -> intervalMs }, fetch)

/** [pollWhileStarted] at a pace that follows what was read last. */
fun <T> CoroutineScope.pollWhileStarted(intervalMs: (T?) -> Long, fetch: suspend () -> T): StateFlow<Loadable<T>> {
    lateinit var state: StateFlow<Loadable<T>>
    state = polling(intervalMs, from = { state.value }, fetch = fetch)
        .stateIn(this, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), Loadable.Loading)
    return state
}

/**
 * Runs [block] each time the screen reaches STARTED and cancels it when the
 * screen stops (`repeatOnLifecycle`). For work driven from the UI side, e.g.
 * a ViewModel's `suspend fun pollUntilDone()`. Restarts when [key] changes.
 */
@Composable
fun RepeatWhileStarted(key: Any?, block: suspend CoroutineScope.() -> Unit) {
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val current = rememberUpdatedState(block)
    LaunchedEffect(lifecycle, key) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { current.value(this) }
    }
}
