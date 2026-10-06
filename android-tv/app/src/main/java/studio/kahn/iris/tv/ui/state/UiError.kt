package studio.kahn.iris.tv.ui.state

import androidx.compose.runtime.Immutable
import java.io.IOException
import kotlinx.coroutines.CancellationException
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import studio.kahn.iris.tv.data.irisError

/**
 * An error as the person reads it: one English sentence, plus the server's
 * error [code] when there is one (`dead_torrent`, `client_outdated`…) for
 * screens that branch on it, and the HTTP [status].
 */
@Immutable
data class UiError(
    val message: String,
    val code: String? = null,
    val status: Int? = null,
) {
    /** Offline or the server did not answer: worth retrying as is. */
    val isNetwork: Boolean get() = status == null && code == NETWORK

    companion object {
        const val NETWORK = "network"
        const val OFFLINE_MESSAGE = "Can't reach the Iris server. Check the connection, then try again."
    }
}

/**
 * The one mapping from a thrown error to [UiError]. Rethrows cancellation:
 * a screen that went away must not report an error.
 */
fun Throwable.toUiError(): UiError = when (this) {
    is CancellationException -> throw this
    is HttpException -> {
        val envelope = irisError()
        UiError(
            message = envelope?.message?.takeIf { it.isNotBlank() } ?: httpMessage(code()),
            code = envelope?.error,
            status = code(),
        )
    }
    is IOException -> UiError(UiError.OFFLINE_MESSAGE, code = UiError.NETWORK)
    // The decoder's own message quotes the payload: never one to show.
    is SerializationException -> UiError(UNREADABLE_MESSAGE)
    else -> UiError(message?.takeIf { it.isNotBlank() } ?: "Something went wrong.")
}

internal const val UNREADABLE_MESSAGE = "The server answered in a form this app does not read. Update the server or the app."

internal fun httpMessage(status: Int): String = when (status) {
    401, 403 -> "This TV is signed out. Pair it again from Settings."
    404 -> "This is no longer on the server."
    // Every method the app sends is routed by a current server: an older one lacks the endpoint.
    405 -> "The Iris server is older than this app and can't do this yet. Update the server."
    426 -> "This app is too old for the server. Update it from Settings."
    429 -> "The server is busy. Try again in a moment."
    in 500..599 -> "The Iris server had a problem. Try again in a moment."
    else -> "The server refused this (error $status)."
}
