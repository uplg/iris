package studio.kahn.iris.tv.ui.state

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.rememberUpdatedState
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.stateIn

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
): Flow<Loadable<T>> = flow {
    var state = from()
    while (true) {
        state = load(state, fetch)
        emit(state)
        delay(intervalMs)
    }
}

/** How long a flow keeps running after the last collector left (a rotation, a quick back-and-forth). */
const val STOP_TIMEOUT_MS = 5_000L

/**
 * A ViewModel's polled read: `val torrents = viewModelScope.pollWhileStarted(5_000) { api.torrents() }`.
 * Collected with `collectAsStateWithLifecycle()`, it polls only while the
 * screen is started (it stops [STOP_TIMEOUT_MS] after Home, the screensaver
 * or another screen on top) and resumes from the value it last showed.
 */
fun <T> CoroutineScope.pollWhileStarted(intervalMs: Long, fetch: suspend () -> T): StateFlow<Loadable<T>> {
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
