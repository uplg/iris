package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.OutlinedTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.CreateCodeRequest
import studio.kahn.iris.tv.data.CreateCodeResponse
import studio.kahn.iris.tv.data.PollResponse
import studio.kahn.iris.tv.data.IrisSession
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.components.IrisWordmark
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.irisAmbient
import android.os.SystemClock
import java.io.IOException
import kotlin.random.Random
import kotlinx.coroutines.CancellationException
import retrofit2.HttpException

/** Pairing poll cadence: a calm 2 s base, exponential backoff up to 15 s on
 *  transient errors, plus jitter so multiple TVs / retries don't synchronise. */
private const val POLL_INTERVAL_MS = 2_000L
private const val POLL_BACKOFF_MAX_MS = 15_000L
private const val POLL_JITTER_MS = 500L

/**
 * Default unauthenticated screen on TV. Asks for the Iris URL, then walks
 * through the device-code pairing dance:
 *
 * 1. POST /api/auth/device/code → server generates a short code (`WX7K-ABCD`)
 *    plus an opaque `device_id`.
 * 2. We display the code + the verification URL (`/account?pair=…`) so the
 *    user can type/scan it on their phone or web browser.
 * 3. We poll `/api/auth/device/poll/{device_id}` every 2s. Once the user
 *    confirms on the web side, the poll response carries Set-Cookie headers
 *    (refresh + access) which our [SessionCookieJar] persists.
 * 4. As soon as `status == "linked"` we navigate Home.
 *
 * No password ever crosses the TV, no D-pad keyboard for credentials.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun PairingScreen(
    container: AppContainer,
    onPaired: () -> Unit,
    onUsePassword: () -> Unit,
) {
    var serverUrl by remember { mutableStateOf("https://iris.kahn.studio") }
    var pairing by remember { mutableStateOf<CreateCodeResponse?>(null) }
    var pending by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(Unit) {
        container.sessionStore.serverUrl.first()?.let { serverUrl = it }
    }

    // Once we have a pairing payload, poll until the user links it on web.
    LaunchedEffect(pairing) {
        val p = pairing ?: return@LaunchedEffect
        val api = container.apiFor(serverUrl)
        // Pre-seed an empty session so the cookie jar has something to
        // overwrite when the poll succeeds.
        container.sessionStore.saveSession(
            IrisSession(serverUrl = serverUrl, email = "", isAdmin = false, cookies = emptyList())
        )
        // Poll only for the code's own server-side lifetime; past it the row is
        // gone and there's nothing left to link. `elapsedRealtime` is monotonic
        // so a wall-clock change can't cut the window short or stretch it.
        val deadline = SystemClock.elapsedRealtime() + p.expiresIn.coerceAtLeast(1) * 1_000
        var backoffMs = POLL_INTERVAL_MS
        while (SystemClock.elapsedRealtime() < deadline) {
            var waitMs = POLL_INTERVAL_MS
            try {
                when (val res = api.pollDeviceCode(p.deviceId.toString())) {
                    is PollResponse.LinkedWrapper -> {
                        // CookieJar persisted Set-Cookie. Save user-visible
                        // fields so the next launch knows who it is.
                        val current = container.sessionStore.session.first()
                        if (current != null) {
                            container.sessionStore.saveSession(
                                current.copy(
                                    email = res.value.user.email,
                                    isAdmin = res.value.user.isAdmin,
                                )
                            )
                        }
                        onPaired()
                        return@LaunchedEffect
                    }
                    is PollResponse.ExpiredWrapper -> {
                        error = "Pairing code expired. Generate a new one."
                        pairing = null
                        return@LaunchedEffect
                    }
                    is PollResponse.PendingWrapper -> backoffMs = POLL_INTERVAL_MS // clean poll → reset
                }
            } catch (e: CancellationException) {
                throw e // never swallow coroutine cancellation (e.g. Cancel pressed)
            } catch (e: HttpException) {
                if (e.code() == 404) {
                    // The code row is gone server-side → treat as expired.
                    error = "Pairing code expired. Generate a new one."
                    pairing = null
                    return@LaunchedEffect
                }
                // 429 / 5xx / any other transient HTTP error: KEEP the code on
                // screen and back off — a single rate-limit or server blip must
                // NOT tear down the pairing session (the old bug, where any
                // exception nulled the code and dumped the user back to the
                // form). Honour Retry-After when the server sends one (429).
                backoffMs = (backoffMs * 2).coerceAtMost(POLL_BACKOFF_MAX_MS)
                val retryAfterMs = e.response()?.headers()?.get("Retry-After")
                    ?.toLongOrNull()?.times(1_000) ?: 0L
                waitMs = maxOf(backoffMs, retryAfterMs)
            } catch (e: IOException) {
                // Network blip: keep the code, back off, keep trying.
                backoffMs = (backoffMs * 2).coerceAtMost(POLL_BACKOFF_MAX_MS)
                waitMs = backoffMs
            }
            delay(waitMs + Random.nextLong(POLL_JITTER_MS))
        }
        // The code's TTL elapsed with no link → expired.
        error = "Pairing code expired. Generate a new one."
        pairing = null
    }

    Box(Modifier.fillMaxSize().background(IrisColors.Background), contentAlignment = Alignment.Center) {
        Box(Modifier.fillMaxSize().background(irisAmbient()))
        if (pairing == null) {
            // Step 1: ask for the URL, generate code.
            Column(
                Modifier.width(540.dp).padding(32.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                IrisWordmark(fontSize = 52.sp)
                Text(
                    "Pair this TV with your Iris account",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = serverUrl,
                    onValueChange = { serverUrl = it.trim() },
                    label = { androidx.compose.material3.Text("Server URL") },
                    singleLine = true,
                    keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                        keyboardType = KeyboardType.Uri
                    ),
                    modifier = Modifier.width(540.dp),
                )
                error?.let {
                    Text(it, color = MaterialTheme.colorScheme.error)
                }
                IrisButton(
                    if (pending) "Generating…" else "Generate pairing code",
                    {
                        if (pending) return@IrisButton
                        pending = true
                        error = null
                        scope.launch {
                            try {
                                container.sessionStore.setServerUrl(serverUrl)
                                val res = container.apiFor(serverUrl)
                                    .createDeviceCode(CreateCodeRequest(kind = "android-tv"))
                                pairing = res
                            } catch (e: Exception) {
                                error = e.message ?: "Failed to create pairing code"
                            } finally {
                                pending = false
                            }
                        }
                    },
                    enabled = !pending && serverUrl.isNotBlank(),
                )
                androidx.compose.material3.TextButton(
                    onClick = onUsePassword,
                    modifier = Modifier.padding(top = 24.dp),
                ) {
                    androidx.compose.material3.Text(
                        "Sign in with email/password instead",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        } else {
            // Step 2: display code, poll in background.
            val code = pairing!!.code
            Column(
                Modifier.padding(32.dp),
                verticalArrangement = Arrangement.spacedBy(20.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    "Pair this TV",
                    style = MaterialTheme.typography.headlineMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                Text(
                    "On your phone or computer, go to:",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    pairing!!.verificationUrl
                        .substringBefore("?pair="),
                    style = MaterialTheme.typography.titleLarge,
                    color = MaterialTheme.colorScheme.primary,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    "and enter this code:",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    code,
                    style = MaterialTheme.typography.displayLarge,
                    color = MaterialTheme.colorScheme.primary,
                    fontWeight = FontWeight.Bold,
                )
                Text(
                    "Waiting for confirmation… (this code expires in 10 minutes)",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                IrisButton("Cancel", { pairing = null }, variant = IrisButtonVariant.Ghost)
            }
        }
    }
}
