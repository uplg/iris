package studio.kahn.iris.tv.ui.state

import androidx.compose.runtime.Immutable
import java.io.IOException
import kotlinx.coroutines.CancellationException
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
    else -> UiError(message?.takeIf { it.isNotBlank() } ?: "Something went wrong.")
}

internal fun httpMessage(status: Int): String = when (status) {
    401, 403 -> "This TV is signed out. Pair it again from Settings."
    404 -> "This is no longer on the server."
    426 -> "This app is too old for the server. Update it from Settings."
    429 -> "The server is busy. Try again in a moment."
    in 500..599 -> "The Iris server had a problem. Try again in a moment."
    else -> "The server refused this (error $status)."
}
