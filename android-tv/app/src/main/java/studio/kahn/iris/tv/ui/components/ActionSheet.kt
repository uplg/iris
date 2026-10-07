package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import studio.kahn.iris.tv.ui.theme.IrisSpace

/**
 * Everything one can do with an item (a card, an episode): its "Hold OK" menu, opened by hold
 * OK on the remote and a long press on a phone. A [SidePanel] titled [title] (an [eyebrow]
 * above it names where the item is), optional [details] about the item, then one
 * button per action, the first one primary and focused.
 *
 * An action that [waits] for the server keeps the sheet open, saying [busyLabel], until the
 * screen's work for it ([inFlight]) has started and ended; the screen says the outcome. Any
 * other action closes the sheet ([onDismiss]) before it runs. While an action travels the
 * others ignore presses. Back or a tap beside the panel closes it. The still-held OK that
 * opened it cannot press the first action.
 */
@Composable
fun <T> ActionSheet(
    title: String,
    actions: List<T>,
    label: (T) -> String,
    onAction: (T) -> Unit,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    eyebrow: String? = null,
    busyLabel: (T) -> String? = { null },
    waits: (T) -> Boolean = { false },
    inFlight: (T) -> Boolean = { false },
    emptyText: String? = null,
    /** Another action of the screen is in flight: the server actions wait (the screen runs one at a time). */
    blocked: Boolean = false,
    details: @Composable ColumnScope.() -> Unit = {},
) {
    val first = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { first.requestFocus() } }
    var waiting by remember { mutableStateOf<T?>(null) }
    var seen by remember { mutableStateOf(false) }
    val pending = waiting
    val travelling = pending != null && inFlight(pending)
    LaunchedEffect(pending, travelling) {
        if (pending == null) return@LaunchedEffect
        if (travelling) seen = true else if (seen) onDismiss()
    }
    SidePanel(
        title = title,
        onDismiss = onDismiss,
        modifier = modifier.dialogKeys(onDismiss),
        footer = "Back closes this panel",
        eyebrow = eyebrow,
    ) {
        Column(Modifier.padding(horizontal = IrisSpace.s4), verticalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
            details()
            if (actions.isEmpty() && emptyText != null) StatusLine(emptyText)
            if (blocked && pending == null) StatusLine("Another action is still on its way to the server: wait for it to finish.", tone = StatusTone.Warn)
            Column(Modifier.padding(top = IrisSpace.s2), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                actions.forEachIndexed { i, action ->
                    ActionButton(
                        label(action),
                        onClick = {
                            if (pending != null || actions.any(inFlight)) return@ActionButton
                            if (blocked && waits(action)) return@ActionButton
                            if (waits(action)) {
                                waiting = action
                                seen = false
                            } else {
                                onDismiss()
                            }
                            onAction(action)
                        },
                        style = if (i == 0) ActionStyle.Primary else ActionStyle.Secondary,
                        busy = action == pending || inFlight(action),
                        busyText = busyLabel(action),
                        modifier = Modifier
                            .fillMaxWidth()
                            .then(if (i == 0) Modifier.focusRequester(first) else Modifier),
                    )
                }
            }
        }
    }
}

/**
 * The remote's Back, taken before Compose sees it: unhandled, its press moves the focus out of
 * the focus group that holds it (`FocusDirection.Exit`) and is spent there, so the back
 * dispatcher only hears the second press. Both halves are swallowed while [enabled]; [onBack]
 * runs on the release, as the platform's Back does. A gesture Back on a phone sends no key:
 * keep a `BackHandler` beside it for that one.
 */
fun Modifier.backKey(enabled: Boolean = true, onBack: () -> Unit): Modifier = onPreviewKeyEvent { event ->
    if (!enabled || event.key != Key.Back) return@onPreviewKeyEvent false
    if (event.type == KeyEventType.KeyUp) onBack()
    true
}

/**
 * The keys of a dialog or a sheet, which may open from a hold of OK: OK still down (its
 * auto-repeats, its release) must not press what takes focus, so Select is swallowed until
 * that press is released; a fresh press (not a repeat) goes through. Back closes ([backKey]).
 */
@Composable
fun Modifier.dialogKeys(onBack: () -> Unit): Modifier {
    var openingPressReleased by remember { mutableStateOf(false) }
    return backKey(onBack = onBack).onPreviewKeyEvent { event ->
        if (openingPressReleased) return@onPreviewKeyEvent false
        val select = event.key == Key.DirectionCenter || event.key == Key.Enter || event.key == Key.NumPadEnter
        if (!select) return@onPreviewKeyEvent false
        val fresh = event.type == KeyEventType.KeyDown && event.nativeKeyEvent.repeatCount == 0
        if (fresh || event.type == KeyEventType.KeyUp) openingPressReleased = true
        !fresh
    }
}
