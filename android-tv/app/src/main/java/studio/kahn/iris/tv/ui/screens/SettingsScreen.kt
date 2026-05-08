package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Button
import androidx.tv.material3.ButtonDefaults
import androidx.tv.material3.Card
import androidx.tv.material3.CardDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.DeviceView
import studio.kahn.iris.tv.data.IrisApi

/**
 * Settings & devices. Lists every refresh token (= active session) and lets
 * the user revoke any of them. Sign-out lives here too — in real-world TV
 * apps you almost never sign out, so it's fine to bury it one screen deep.
 *
 * No way to enroll new devices from the TV: pairing is initiated on the TV
 * (PairingScreen) and confirmed on the web. This screen only revokes.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun SettingsScreen(
    container: AppContainer,
    onSignOut: () -> Unit,
    onBack: () -> Unit,
) {
    var devices by remember { mutableStateOf<List<DeviceView>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var loadVersion by remember { mutableIntStateOf(0) }
    var serverUrl by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(loadVersion) {
        loading = true
        error = null
        try {
            val url = container.sessionStore.serverUrl.first()
                ?: run { error = "Not signed in"; loading = false; return@LaunchedEffect }
            serverUrl = url
            val api: IrisApi = container.apiFor(url)
            devices = api.listDevices()
        } catch (e: Exception) {
            error = e.message ?: "Failed to load devices"
        } finally {
            loading = false
        }
    }

    Column(
        Modifier.fillMaxSize().padding(40.dp),
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        Text(
            "Settings",
            style = MaterialTheme.typography.headlineLarge,
            fontWeight = FontWeight.SemiBold,
            color = MaterialTheme.colorScheme.primary,
        )

        // Server card.
        Card(
            onClick = {},
            modifier = Modifier.fillMaxWidth(),
            shape = CardDefaults.shape(shape = RoundedCornerShape(12.dp)),
        ) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("Server".uppercase(), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(serverUrl ?: "—", style = MaterialTheme.typography.bodyLarge)
            }
        }

        // Devices section.
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Paired devices".uppercase(), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Button(
                onClick = { loadVersion++ },
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 6.dp),
            ) { Text("Refresh") }
        }

        Box(Modifier.weight(1f)) {
            when {
                loading -> Text("Loading devices…", color = MaterialTheme.colorScheme.onSurfaceVariant)
                error != null -> Text(error!!, color = MaterialTheme.colorScheme.error)
                devices.isEmpty() -> Text(
                    "No devices paired yet — generate a code from the TV pairing screen.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                else -> LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    items(devices, key = { it.jti }) { d ->
                        DeviceRow(
                            device = d,
                            onRevoke = {
                                scope.launch {
                                    runCatching {
                                        val url = container.sessionStore.serverUrl.first() ?: return@launch
                                        container.apiFor(url).revokeDevice(d.jti)
                                    }
                                    loadVersion++
                                }
                            },
                        )
                    }
                }
            }
        }

        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Button(
                onClick = onBack,
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
            ) { Text("Back") }
            Box(Modifier.weight(1f))
            Button(
                onClick = {
                    scope.launch {
                        runCatching {
                            container.sessionStore.serverUrl.first()
                                ?.let { container.apiFor(it).logout() }
                        }
                        container.sessionStore.clear()
                        onSignOut()
                    }
                },
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
            ) { Text("Sign out") }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun DeviceRow(
    device: DeviceView,
    onRevoke: () -> Unit,
) {
    Card(
        onClick = {},
        modifier = Modifier.fillMaxWidth(),
        shape = CardDefaults.shape(shape = RoundedCornerShape(8.dp)),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    device.label ?: device.kind ?: device.jti.take(8),
                    style = MaterialTheme.typography.bodyLarge,
                )
                Text(
                    listOfNotNull(
                        device.kind,
                        "issued ${device.issuedAt.take(10)}",
                        "expires ${device.expiresAt.take(10)}",
                    ).joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Button(
                onClick = onRevoke,
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 6.dp),
            ) { Text("Revoke") }
        }
    }
}
