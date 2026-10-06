package studio.kahn.iris.tv.ui.components

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged

/**
 * Where focus comes back to on a screen: the item left when coming back (from a dialog, a
 * panel, another screen), or its neighbour when the item disappeared. Items are known by a
 * stable key (the one their lazy list uses): tag each with [Modifier.focusReturn].
 *
 * Make it with [rememberFocusReturn], which also runs the returns asked by [returnTo].
 */
@Stable
class FocusReturn internal constructor(last: String?) {
    private val requesters = HashMap<String, FocusRequester>()
    private var pending by mutableStateOf<List<String>?>(null)

    /** The key of the item focused last (kept across a navigation away and back). */
    var last: String? = last
        private set

    internal var onLast: (String) -> Unit = {}

    internal fun focused(key: String) {
        last = key
        onLast(key)
    }

    fun requester(key: String): FocusRequester = requesters.getOrPut(key) { FocusRequester() }

    /**
     * Focuses the first of [keys] whose item is composed now; false when none is (focus does
     * not move). A key never tagged counts as absent.
     */
    fun focus(keys: List<String>): Boolean = keys.any { k ->
        requesters[k]?.let { r -> runCatching { r.requestFocus(FocusDirection.Enter) }.getOrDefault(false) } == true
    }

    /** [focus] on the item focused last. */
    fun focusLast(): Boolean = last?.let { focus(listOf(it)) } == true

    /**
     * Asks focus back on [key] once the screen has settled (the next frame): else on the items
     * after it in [among] (the list as it was with [key] in it), then those before it, nearest
     * first, else on the fallback of [rememberFocusReturn]. [leaving]: the item is about to go
     * (the server is removing it), so its neighbours come first.
     */
    fun returnTo(key: String, among: List<String> = emptyList(), leaving: Boolean = false) {
        val near = neighboursOf(among, key)
        pending = if (leaving) near + key else listOf(key) + near
    }

    internal fun take(): List<String>? = pending.also { pending = null }

    internal val asked: List<String>? get() = pending
}

/** The items around [key] in [keys], nearest first: those after it, then those before it. */
fun <T> neighboursOf(keys: List<T>, key: T): List<T> {
    val i = keys.indexOf(key)
    if (i < 0) return emptyList()
    return keys.drop(i + 1) + keys.take(i).reversed()
}

/**
 * A [FocusReturn] for this screen, its last key saved across navigation ([initial] the first
 * time). A return [FocusReturn.returnTo] asked lands on [fallback] when no item it names is
 * composed.
 */
@Composable
fun rememberFocusReturn(initial: String? = null, fallback: FocusRequester? = null): FocusReturn {
    var saved by rememberSaveable { mutableStateOf(initial) }
    val focus = remember { FocusReturn(saved) }
    focus.onLast = { saved = it }
    val asked = focus.asked
    LaunchedEffect(asked) {
        if (asked == null) return@LaunchedEffect
        withFrameNanos { }
        val keys = focus.take() ?: return@LaunchedEffect
        if (!focus.focus(keys) && fallback != null) runCatching { fallback.requestFocus() }
    }
    return focus
}

/** Tags an item of [focus] by its [key]: it can be focused again, and it is remembered when focused. */
fun Modifier.focusReturn(focus: FocusReturn, key: String): Modifier =
    focusRequester(focus.requester(key)).onFocusChanged { if (it.hasFocus) focus.focused(key) }
