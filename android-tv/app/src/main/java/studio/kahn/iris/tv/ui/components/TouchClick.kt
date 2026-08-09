package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput

/**
 * Touch-tap shim for tv-material components. `androidx.tv:tv-material`'s
 * internal `tvClickable` modifier is focus + D-pad key events ONLY — it
 * installs no pointer handling at all, so on a touch device (the APK
 * installs fine on phones by design, see the manifest note) every
 * Surface/Card/Button draws but ignores taps. Chaining this modifier
 * next to the component's own `onClick` makes the same lambda fire on
 * tap (and long-press where the component has one) without touching
 * D-pad behaviour or visuals on TV.
 *
 * The lambdas are used as `pointerInput` keys so a recomposition that
 * changes them restarts the detector — stale-closure-safe inside lazy
 * lists at the cost of a cheap detector restart.
 */
fun Modifier.touchClick(
    enabled: Boolean = true,
    onLongClick: (() -> Unit)? = null,
    onClick: () -> Unit,
): Modifier = pointerInput(enabled, onClick, onLongClick) {
    if (!enabled) return@pointerInput
    detectTapGestures(
        onTap = { onClick() },
        onLongPress = onLongClick?.let { long -> { long() } },
    )
}
