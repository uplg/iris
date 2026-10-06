package studio.kahn.iris.tv.ui.screens

import android.os.SystemClock
import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlin.random.Random
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import retrofit2.HttpException
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.CreateCodeRequest
import studio.kahn.iris.tv.data.IrisSession
import studio.kahn.iris.tv.data.PollResponse
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.failedWith
import studio.kahn.iris.tv.ui.state.toUiError

/** A device code on screen, waiting to be confirmed on the web. */
@Immutable
data class PairingCode(
    val code: String,
    /** Where to type it, without the `?pair=` the QR flow appends. */
    val verificationUrl: String,
    val deviceId: String,
    /** [SystemClock.elapsedRealtime] past which the server forgets the code. */
    val expiresAtMs: Long,
)

/**
 * Everything the pairing screen draws. [code] null = the server-URL form;
 * Loading = the code is being created; Failed = it could not be (the form
 * shows why); Ready = the code is shown and polled; Stale = still shown,
 * but the last poll failed (the screen says it keeps trying).
 */
@Immutable
data class PairingUiState(
    val serverUrl: String = DEFAULT_SERVER_URL,
    val code: Loadable<PairingCode>? = null,
    /** Why the form is back (the code expired). */
    val notice: String? = null,
    /** The TV is linked: navigate on. */
    val linked: Boolean = false,
) {
    val showsCode: Boolean get() = code?.valueOrNull != null
    val generating: Boolean get() = code == Loadable.Loading
}

const val DEFAULT_SERVER_URL = "https://iris.kahn.studio"

/**
 * Device-code pairing (reference ViewModel for the screen pattern):
 * 1. [generate]: POST /api/auth/device/code, the code is shown.
 * 2. [pollUntilLinked], run by the screen only while it is started: polls
 *    /api/auth/device/poll/{id} until the person confirms on the web (the
 *    poll response carries the session cookies, persisted by the cookie
 *    jar), the code expires, or the screen stops.
 * No password crosses the TV.
 */
class PairingViewModel(
    private val container: AppContainer,
    private val now: () -> Long = SystemClock::elapsedRealtime,
) : ViewModel() {
    private val mutable = MutableStateFlow(PairingUiState())
    val state: StateFlow<PairingUiState> = mutable.asStateFlow()
    private var urlEdited = false

    init {
        viewModelScope.launch {
            val stored = container.sessionStore.serverUrl.first()
            if (stored != null && !urlEdited) mutable.update { it.copy(serverUrl = stored) }
        }
    }

    fun onServerUrlChange(url: String) {
        urlEdited = true
        mutable.update { it.copy(serverUrl = url.trim()) }
    }

    fun generate() {
        val current = mutable.value
        if (current.generating || current.serverUrl.isBlank()) return
        mutable.update { it.copy(code = Loadable.Loading, notice = null) }
        viewModelScope.launch {
            val url = current.serverUrl
            val next = try {
                container.sessionStore.setServerUrl(url)
                val res = container.apiFor(url).createDeviceCode(CreateCodeRequest(kind = "android-tv"))
                // An empty session for the cookie jar to fill when the poll succeeds.
                container.sessionStore.saveSession(
                    IrisSession(serverUrl = url, email = "", isAdmin = false, cookies = emptyList()),
                )
                Loadable.Ready(
                    PairingCode(
                        code = res.code,
                        verificationUrl = res.verificationUrl.substringBefore("?pair="),
                        deviceId = res.deviceId.toString(),
                        expiresAtMs = now() + res.expiresIn.coerceAtLeast(1) * 1_000,
                    ),
                )
            } catch (e: Exception) {
                Loadable.Failed(e.toUiError())
            }
            mutable.update { it.copy(code = next) }
        }
    }

    fun cancel() {
        mutable.update { it.copy(code = null, notice = null) }
    }

    /**
     * Polls until linked, expired or cancelled. Transient failures (429, 5xx,
     * network) keep the code on screen as Stale and back off; a 404 means
     * the server dropped the code.
     */
    suspend fun pollUntilLinked() {
        var backoffMs = POLL_INTERVAL_MS
        while (true) {
            val code = mutable.value.code?.valueOrNull ?: return
            if (now() >= code.expiresAtMs) return expire()
            val api = container.apiFor(mutable.value.serverUrl)
            var waitMs = POLL_INTERVAL_MS
            try {
                when (val res = api.pollDeviceCode(code.deviceId)) {
                    is PollResponse.LinkedWrapper -> {
                        container.sessionStore.session.first()?.let { session ->
                            container.sessionStore.saveSession(
                                session.copy(email = res.value.user.email, isAdmin = res.value.user.isAdmin),
                            )
                        }
                        mutable.update { it.copy(linked = true) }
                        return
                    }
                    is PollResponse.ExpiredWrapper -> return expire()
                    is PollResponse.PendingWrapper -> {
                        backoffMs = POLL_INTERVAL_MS
                        mutable.update { it.copy(code = Loadable.Ready(code)) }
                    }
                }
            } catch (e: Exception) {
                val error = e.toUiError()
                if (error.status == 404) return expire()
                backoffMs = nextBackoff(backoffMs)
                waitMs = maxOf(backoffMs, (e as? HttpException).retryAfterMs())
                mutable.update { it.copy(code = it.code?.failedWith(error)) }
            }
            delay(waitMs + Random.nextLong(POLL_JITTER_MS))
        }
    }

    private fun expire() {
        mutable.update { it.copy(code = null, notice = "The pairing code expired. Generate a new one.") }
    }
}

private const val POLL_INTERVAL_MS = 2_000L
private const val POLL_BACKOFF_MAX_MS = 15_000L
private const val POLL_JITTER_MS = 500L

/** Doubles a poll delay, capped at 15 s. */
internal fun nextBackoff(currentMs: Long): Long = (currentMs * 2).coerceAtMost(POLL_BACKOFF_MAX_MS)

private fun HttpException?.retryAfterMs(): Long =
    this?.response()?.headers()?.get("Retry-After")?.toLongOrNull()?.times(1_000) ?: 0L
