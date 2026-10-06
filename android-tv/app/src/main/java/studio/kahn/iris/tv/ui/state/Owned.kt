package studio.kahn.iris.tv.ui.state

import androidx.compose.runtime.RememberObserver

/**
 * A [value] the composition owns, remembered as is: [release]d when it is forgotten (after the
 * effects remembered later, which may still use it, are disposed) or when the composition that
 * built it is abandoned, never applied (a [androidx.compose.runtime.DisposableEffect] alone
 * would leak it then).
 */
class Owned<T>(val value: T, private val release: (T) -> Unit) : RememberObserver {
    override fun onRemembered() = Unit

    override fun onForgotten() = release(value)

    override fun onAbandoned() = release(value)
}
