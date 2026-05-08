package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.tv.material3.Button
import androidx.tv.material3.ButtonDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.DeviceCodeRequest
import studio.kahn.iris.tv.data.DeviceCodeResponse
import studio.kahn.iris.tv.data.IrisSession

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
    var pairing by remember { mutableStateOf<DeviceCodeResponse?>(null) }
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
        while (true) {
            try {
                val res = api.pollDeviceCode(p.deviceId)
                when (res.status) {
                    "linked" -> {
                        // CookieJar persisted Set-Cookie. Save user-visible
                        // fields so the next launch knows who it is.
                        val current = container.sessionStore.session.first()
                        if (current != null && res.user != null) {
                            container.sessionStore.saveSession(
                                current.copy(email = res.user.email, isAdmin = res.user.isAdmin)
                            )
                        }
                        onPaired()
                        return@LaunchedEffect
                    }
                    "expired" -> {
                        error = "Pairing code expired. Generate a new one."
                        pairing = null
                        return@LaunchedEffect
                    }
                }
            } catch (e: Exception) {
                error = e.message ?: "Polling failed"
                pairing = null
                return@LaunchedEffect
            }
            delay(2_000)
        }
    }

    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        if (pairing == null) {
            // Step 1: ask for the URL, generate code.
            Column(
                Modifier.width(540.dp).padding(32.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    "Iris  /",
                    style = MaterialTheme.typography.displayMedium,
                    color = MaterialTheme.colorScheme.primary,
                )
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
                Button(
                    onClick = {
                        if (pending) return@Button
                        pending = true
                        error = null
                        scope.launch {
                            try {
                                container.sessionStore.setServerUrl(serverUrl)
                                val res = container.apiFor(serverUrl)
                                    .createDeviceCode(DeviceCodeRequest(kind = "android-tv"))
                                pairing = res
                            } catch (e: Exception) {
                                error = e.message ?: "Failed to create pairing code"
                            } finally {
                                pending = false
                            }
                        }
                    },
                    enabled = !pending && serverUrl.isNotBlank(),
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 24.dp, vertical = 14.dp),
                ) {
                    Text(if (pending) "Generating…" else "Generate pairing code")
                }
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
                Button(
                    onClick = { pairing = null },
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                ) {
                    Text("Cancel")
                }
            }
        }
    }
}
