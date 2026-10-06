package studio.kahn.iris.tv.ui.update

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.SystemUpdate
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import studio.kahn.iris.tv.data.AppUpdater
import studio.kahn.iris.tv.data.AppUpdates
import studio.kahn.iris.tv.data.UpdateState
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Meter
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.format.formatSize
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** What the update's controls do, wherever they are ([AppUpdates.actions]). */
@Immutable
class UpdateActions(
    val download: () -> Unit = {},
    val cancel: () -> Unit = {},
    val install: () -> Unit = {},
    val later: () -> Unit = {},
)

fun AppUpdates.actions(): UpdateActions = UpdateActions(::download, ::cancel, ::install, ::later)

/**
 * The update's one button, which changes with the download so the focus stays on it:
 * [idle] (download and install), Cancel while it downloads, Install once it is downloaded,
 * Try again after a failure.
 */
@Composable
fun UpdateButton(
    progress: AppUpdater.Progress?,
    actions: UpdateActions,
    modifier: Modifier = Modifier,
    idle: String = "Update now",
    size: ActionSize = ActionSize.Regular,
) {
    when (progress) {
        AppUpdater.Progress.Connecting, is AppUpdater.Progress.Downloading ->
            ActionButton("Cancel the download", actions.cancel, modifier, style = ActionStyle.Secondary, size = size)
        is AppUpdater.Progress.Ready ->
            ActionButton("Install the update", actions.install, modifier, icon = Icons.Rounded.SystemUpdate, size = size)
        is AppUpdater.Progress.Failed ->
            ActionButton("Try again", actions.download, modifier, icon = Icons.Rounded.SystemUpdate, size = size)
        null -> ActionButton(idle, actions.download, modifier, icon = Icons.Rounded.SystemUpdate, size = size)
    }
}

/** The download in words (and a meter while its size is known), announced as it changes. */
@Composable
fun UpdateProgress(progress: AppUpdater.Progress?, modifier: Modifier = Modifier) {
    val live = modifier.semantics { liveRegion = LiveRegionMode.Polite }
    when (progress) {
        null -> Unit
        AppUpdater.Progress.Connecting -> StatusLine("Connecting to the download…", live, tone = StatusTone.Busy)
        is AppUpdater.Progress.Downloading -> Column(live, verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
            val share = if (progress.total > 0) (progress.bytes.toFloat() / progress.total).coerceIn(0f, 1f) else null
            Text(
                if (share != null) {
                    "Downloading · ${(share * 100).toInt()} % (${formatSize(progress.bytes)} of ${formatSize(progress.total)})"
                } else {
                    "Downloading · ${formatSize(progress.bytes)}, size unknown"
                },
                style = IrisType.meta,
                color = IrisColor.ink,
            )
            if (share != null) Meter(share)
        }
        is AppUpdater.Progress.Ready -> StatusLine(
            "Downloaded. The installer asks you to confirm; if it doesn’t show, press Install the update.",
            live,
            tone = StatusTone.Ok,
            maxLines = 2,
        )
        is AppUpdater.Progress.Failed -> StatusLine("The update failed: ${progress.message}", live, tone = StatusTone.Down, maxLines = 2)
    }
}

/**
 * Hands a finished download to the system installer, wherever it was started from. The
 * installer needs us in front: a file ready while the TV was on its home screen or the
 * screensaver waits for the next resume. A launch the system dropped without covering us is
 * fired again, twice at most; once the installer covered us, never again (that would reopen
 * it over a cancel). The screen stays on while the download runs.
 */
@Composable
fun UpdateInstaller(updates: AppUpdates) {
    val state by updates.state.collectAsStateWithLifecycle()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val view = LocalView.current
    val ready = (state.progress as? AppUpdater.Progress.Ready)?.file

    val active = state.downloading || ready != null
    DisposableEffect(active) {
        view.keepScreenOn = active
        onDispose { view.keepScreenOn = false }
    }

    var covered by remember(ready) { mutableStateOf(false) }
    DisposableEffect(lifecycle, ready) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_PAUSE) covered = true
        }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }
    LaunchedEffect(ready) {
        if (ready == null) return@LaunchedEffect
        lifecycle.currentStateFlow.first { it.isAtLeast(Lifecycle.State.RESUMED) }
        updates.install()
        repeat(INSTALL_RETRIES) {
            delay(INSTALL_RETRY_MS)
            if (covered) return@LaunchedEffect
            updates.install()
        }
    }
}

private const val INSTALL_RETRIES = 2
private const val INSTALL_RETRY_MS = 2_500L

/** "Iris 1.6.0 is available · you have 1.5.0", or the installed version alone when the latest is unknown. */
fun versionsLine(state: UpdateState): String = versionsLine(state.available, state.installed)

fun versionsLine(latest: String?, installed: String): String =
    if (latest != null) "Iris $latest is available · you have $installed" else "You have Iris $installed"
