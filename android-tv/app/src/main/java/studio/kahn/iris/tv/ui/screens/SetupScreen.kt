package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.KeyboardOptions
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
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.IrisSession
import studio.kahn.iris.tv.data.LoginRequest
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisWordmark
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.irisAmbient

/**
 * Fallback re-pair / direct login screen. The user types their Iris URL +
 * email + password; on success we cache the session cookies via [AppContainer]
 * and navigate Home.
 *
 * Most users will reach this only if they explicitly click "Sign in with
 * email/password" on the [PairingScreen] — that's the preferred path.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun SetupScreen(
    container: AppContainer,
    onAuthenticated: () -> Unit,
) {
    var serverUrl by remember { mutableStateOf("https://iris.kahn.studio") }
    var email by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var pending by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(Unit) {
        container.sessionStore.serverUrl.first()?.let { serverUrl = it }
    }

    Box(Modifier.fillMaxSize().background(IrisColors.Background), contentAlignment = Alignment.Center) {
        Box(Modifier.fillMaxSize().background(irisAmbient()))
        Column(
            Modifier.width(540.dp).padding(32.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            IrisWordmark(fontSize = 52.sp)
            Text(
                "Sign in to your Iris server",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            OutlinedTextField(
                value = serverUrl,
                onValueChange = { serverUrl = it.trim() },
                label = { androidx.compose.material3.Text("Server URL") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                modifier = Modifier.width(540.dp),
            )
            OutlinedTextField(
                value = email,
                onValueChange = { email = it.trim() },
                label = { androidx.compose.material3.Text("Email") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                modifier = Modifier.width(540.dp),
            )
            OutlinedTextField(
                value = password,
                onValueChange = { password = it },
                label = { androidx.compose.material3.Text("Password") },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                modifier = Modifier.width(540.dp),
            )

            error?.let {
                Text(it, color = MaterialTheme.colorScheme.error)
            }

            IrisButton(
                if (pending) "Signing in…" else "Sign in",
                {
                    if (pending) return@IrisButton
                    pending = true
                    error = null
                    scope.launch {
                        try {
                            container.sessionStore.setServerUrl(serverUrl)
                            container.sessionStore.saveSession(
                                IrisSession(
                                    serverUrl = serverUrl,
                                    email = email,
                                    isAdmin = false,
                                    cookies = emptyList(),
                                )
                            )
                            val api = container.apiFor(serverUrl)
                            val user = api.login(LoginRequest(email, password))
                            // CookieJar already persisted Set-Cookie. Update
                            // the user-visible fields (email, admin badge).
                            val current = container.sessionStore.session.first()
                            if (current != null) {
                                container.sessionStore.saveSession(
                                    current.copy(email = user.email, isAdmin = user.isAdmin)
                                )
                            }
                            onAuthenticated()
                        } catch (e: Exception) {
                            error = e.message ?: "Login failed"
                        } finally {
                            pending = false
                        }
                    }
                },
                enabled = !pending && email.isNotBlank() && password.isNotBlank() && serverUrl.isNotBlank(),
            )
        }
    }
}
