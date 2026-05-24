package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Surface
import androidx.tv.material3.SurfaceDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.BuildConfig
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.AppUpdater
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Spacing

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
    var serverUrl by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(Unit) {
        serverUrl = container.sessionStore.serverUrl.first()
    }

    val layout = LocalTvLayout.current
    val cardShape = RoundedCornerShape(studio.kahn.iris.tv.ui.theme.Radius.poster)
    Column(
        Modifier
            .fillMaxSize()
            .padding(
                horizontal = layout.gutterHorizontal,
                vertical = layout.gutterVertical,
            ),
        verticalArrangement = Arrangement.spacedBy(Spacing.xl),
    ) {
        Text(
            "Settings",
            style = MaterialTheme.typography.displaySmall,
            color = MaterialTheme.colorScheme.onSurface,
        )

        // Server card — passive (non-focusable) Surface so it never zooms or
        // overflows the screen on focus.
        Surface(
            modifier = Modifier.fillMaxWidth(),
            shape = cardShape,
            colors = SurfaceDefaults.colors(containerColor = IrisColors.Card),
        ) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Eyebrow("Server")
                Text(serverUrl ?: "—", style = MaterialTheme.typography.bodyLarge)
            }
        }

        UpdaterCard(container = container)

        // Push the actions to the bottom of the screen so they stay visible.
        Box(Modifier.weight(1f))

        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            IrisButton("Back", onBack, variant = IrisButtonVariant.Ghost)
            Box(Modifier.weight(1f))
            IrisButton(
                "Sign out",
                {
                    scope.launch {
                        runCatching {
                            container.sessionStore.serverUrl.first()
                                ?.let { container.apiFor(it).logout() }
                        }
                        container.sessionStore.clear()
                        onSignOut()
                    }
                },
                variant = IrisButtonVariant.Ghost,
            )
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun UpdaterCard(container: AppContainer) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var state by remember { mutableStateOf<AppUpdater.Progress?>(null) }
    var job by remember { mutableStateOf<Job?>(null) }
    var versionStatus by remember {
        mutableStateOf<AppUpdater.VersionStatus>(AppUpdater.VersionStatus.Unknown)
    }

    DisposableEffect(Unit) {
        onDispose { job?.cancel() }
    }

    // Best-effort version probe on screen entry. Failure leaves the
    // status at Unknown and the UI shows "version check unavailable" —
    // never blocks the Download button.
    LaunchedEffect(Unit) {
        val latest = AppUpdater.fetchLatestVersion(container.okHttpClient)
        versionStatus = AppUpdater.versionStatus(BuildConfig.VERSION_NAME, latest)
    }

    // NOT a Card here: a clickable Card on TV grabs D-pad focus and swallows
    // OK presses, so the inner Download Button never fires. Use a passive
    // Surface so focus traverses straight to the button.
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = SurfaceDefaults.colors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant,
        ),
    ) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Eyebrow("App update")
            Text(
                "Installed version ${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})",
                style = MaterialTheme.typography.bodyLarge,
            )
            VersionStatusLine(versionStatus)
            Text(
                "Pulls the latest APK from ${AppUpdater.APK_URL} and hands it to the system installer. The TV will ask you to confirm.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            UpdaterStatus(state)

            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                val downloading = state is AppUpdater.Progress.Connecting
                    || state is AppUpdater.Progress.Downloading
                IrisButton(
                    if (downloading) "Cancel" else "Download & install",
                    {
                        if (job?.isActive == true) {
                            job?.cancel()
                            state = null
                            return@IrisButton
                        }
                        if (!AppUpdater.canInstallPackages(context)) {
                            // Punt to the system "install unknown apps"
                            // settings before even hitting the network.
                            // The user comes back, hits Download again.
                            AppUpdater.openInstallPermissionSettings(context)
                            state = AppUpdater.Progress.Failed(
                                "grant Install unknown apps for Iris TV, then try again",
                            )
                            return@IrisButton
                        }
                        state = AppUpdater.Progress.Connecting
                        job = scope.launch {
                            AppUpdater.downloadApk(context, container.okHttpClient)
                                .collect { p ->
                                    state = p
                                    if (p is AppUpdater.Progress.Ready) {
                                        AppUpdater.requestInstall(context, p.file)
                                    }
                                }
                        }
                    },
                )
                if (state is AppUpdater.Progress.Ready) {
                    val ready = state as AppUpdater.Progress.Ready
                    IrisButton(
                        "Reopen installer",
                        { AppUpdater.requestInstall(context, ready.file) },
                        variant = IrisButtonVariant.Ghost,
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun VersionStatusLine(status: AppUpdater.VersionStatus) {
    when (status) {
        AppUpdater.VersionStatus.Unknown -> Text(
            "Latest available · checking…",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        is AppUpdater.VersionStatus.UpToDate -> Text(
            "Up to date (latest: ${status.latest})",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.primary,
        )
        is AppUpdater.VersionStatus.UpdateAvailable -> Text(
            "Update available: ${status.latest}",
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.SemiBold,
            color = MaterialTheme.colorScheme.tertiary,
        )
    }
}

@Composable
private fun UpdaterStatus(state: AppUpdater.Progress?) {
    when (state) {
        null -> Unit
        is AppUpdater.Progress.Connecting ->
            Text("Connecting…", style = MaterialTheme.typography.bodyMedium)
        is AppUpdater.Progress.Downloading -> {
            val pct = if (state.total > 0) (state.bytes.toFloat() / state.total).coerceIn(0f, 1f) else null
            val label = if (pct != null) {
                "Downloading · ${(pct * 100).toInt()}% (${formatBytes(state.bytes)} / ${formatBytes(state.total)})"
            } else {
                "Downloading · ${formatBytes(state.bytes)} (size unknown)"
            }
            Text(label, style = MaterialTheme.typography.bodyMedium)
            if (pct != null) {
                LinearProgressIndicator(
                    progress = { pct },
                    modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
                )
            } else {
                LinearProgressIndicator(
                    modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
                )
            }
        }
        is AppUpdater.Progress.Ready ->
            Text(
                "Downloaded — opening installer.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.primary,
            )
        is AppUpdater.Progress.Failed ->
            Text(
                state.message,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.error,
            )
    }
}

private fun formatBytes(b: Long): String {
    if (b < 0) return "?"
    val mb = b / 1_000_000.0
    if (mb >= 1.0) return String.format(java.util.Locale.ROOT, "%.1f MB", mb)
    val kb = b / 1_000.0
    if (kb >= 1.0) return String.format(java.util.Locale.ROOT, "%.0f KB", kb)
    return "$b B"
}
