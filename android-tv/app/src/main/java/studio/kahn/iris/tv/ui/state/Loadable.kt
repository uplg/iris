package studio.kahn.iris.tv.ui.state

import androidx.compose.runtime.Immutable

/**
 * What a screen knows about one server read. Four states, no others:
 *
 * - [Loading]: nothing yet (show [studio.kahn.iris.tv.ui.components.LoadingState]).
 * - [Failed]: nothing, and the read failed (show the error with a Retry).
 * - [Ready]: the value.
 * - [Stale]: a value, but the LAST refresh failed. Keep showing the value
 *   (never blank a screen over a blip) and say it may be out of date.
 *
 * Never shows a value ahead of the server: a write is reflected only once
 * the server answered (no optimistic UI).
 */
@Immutable
sealed interface Loadable<out T> {
    data object Loading : Loadable<Nothing>
    data class Failed(val error: UiError) : Loadable<Nothing>
    data class Ready<T>(val value: T) : Loadable<T>
    data class Stale<T>(val value: T, val error: UiError) : Loadable<T>

    /** The value when there is one (ready or stale). */
    val valueOrNull: T?
        get() = when (this) {
            is Ready -> value
            is Stale -> value
            Loading, is Failed -> null
        }

    /** The error to show: the failure, or why the value is stale. */
    val errorOrNull: UiError?
        get() = when (this) {
            is Failed -> error
            is Stale -> error
            Loading, is Ready -> null
        }
}

/**
 * A failed (re)read: a value already shown becomes [Loadable.Stale], an
 * empty state becomes [Loadable.Failed].
 */
fun <T> Loadable<T>.failedWith(error: UiError): Loadable<T> = when (this) {
    is Loadable.Ready -> Loadable.Stale(value, error)
    is Loadable.Stale -> Loadable.Stale(value, error)
    Loadable.Loading, is Loadable.Failed -> Loadable.Failed(error)
}

/** Maps the value, keeping the state. */
inline fun <T, R> Loadable<T>.map(transform: (T) -> R): Loadable<R> = when (this) {
    is Loadable.Ready -> Loadable.Ready(transform(value))
    is Loadable.Stale -> Loadable.Stale(transform(value), error)
    Loadable.Loading -> Loadable.Loading
    is Loadable.Failed -> this
}
