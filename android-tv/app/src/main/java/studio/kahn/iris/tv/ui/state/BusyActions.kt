package studio.kahn.iris.tv.ui.state

import androidx.compose.runtime.Immutable
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.ui.components.Notice

/** The actions of a screen in flight (their keys) and what the last one ended with. */
@Immutable
data class ActionsState(val busy: Set<String> = emptySet(), val notice: Notice? = null) {
    /** The one action in flight, on a screen that runs one at a time. */
    val busyKey: String? get() = busy.firstOrNull()
}

/**
 * A screen's actions on the server (delete, pause, mark as watched…): each runs once at a
 * time by its key ([oneAtATime]: one for the whole screen), shows as busy while it travels,
 * and says how it ended once the server answered: the [Notice] its block returns, or the
 * error in words. Nothing is shown as done before that (no optimistic UI).
 */
class BusyActions(private val scope: CoroutineScope, private val oneAtATime: Boolean = false) {
    private val mutable = MutableStateFlow(ActionsState())
    val state: StateFlow<ActionsState> = mutable.asStateFlow()

    /** Whether an action keyed [key] would be refused now. */
    fun refuses(key: String): Boolean = mutable.value.busy.let { key in it || (oneAtATime && it.isNotEmpty()) }

    /**
     * Runs [block] as [key]; [onSuccess] once it went through. False when it was refused (the
     * same action, or on a one-at-a-time screen any action, is still in flight).
     */
    fun run(key: String, onSuccess: (() -> Unit)? = null, block: suspend CoroutineScope.() -> Notice?): Boolean {
        if (refuses(key)) return false
        mutable.update { it.copy(busy = it.busy + key, notice = null) }
        scope.launch {
            var ok = false
            val notice = try {
                coroutineScope { block() }.also { ok = true }
            } catch (e: Exception) {
                Notice(e.toUiError().message, failed = true)
            }
            mutable.update { it.copy(busy = it.busy - key, notice = notice) }
            if (ok) onSuccess?.invoke()
        }
        return true
    }

    /** [run], its success said in [done]'s words. */
    fun runSaying(key: String, onSuccess: (() -> Unit)? = null, done: suspend CoroutineScope.() -> String?): Boolean =
        run(key, onSuccess) { done()?.let { Notice(it, failed = false) } }

    /** Says [notice] (or clears it) without an action. */
    fun say(notice: Notice?) = mutable.update { it.copy(notice = notice) }
}
