package studio.kahn.iris.tv.ui.nav

import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.SystemUpdate
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.IrisWordmark
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.KeyHints
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.dialogCard
import studio.kahn.iris.tv.ui.components.touchClick
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * The server refused this app as too old (HTTP 426): everything but
 * Settings, where the updater lives, is covered. The screen below stays
 * composed (its state is kept for after the update) but cannot take focus
 * or taps. Back leaves the app.
 */
@Composable
fun ClientOutdatedOverlay(installedVersion: String, onOpenSettings: () -> Unit) {
    val activity = LocalActivity.current
    BackHandler { activity?.moveTaskToBack(true) }
    val open = remember { FocusRequester() }
    val held = remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { runCatching { open.requestFocus() } }
    val layout = IrisLayout.current
    Box(
        Modifier
            .fillMaxSize()
            .background(IrisColor.ground)
            .touchClick {}
            .semantics { paneTitle = "Update Iris" }
            .focusProperties { onExit = { cancelFocusChange() } }
            // A screen below asking for the focus late (Home's hero) would press a hidden
            // button: the focus comes straight back here.
            .onFocusChanged {
                if (it.hasFocus) held.value = true else if (held.value) runCatching { open.requestFocus() }
            }
            .focusGroup()
            .padding(layout.safePadding),
    ) {
        IrisWordmark(Modifier.align(Alignment.TopStart))
        Column(
            Modifier
                .align(Alignment.Center)
                .widthIn(max = 480.dp)
                .dialogCard()
                .padding(IrisSpace.s8)
                .semantics { liveRegion = LiveRegionMode.Polite },
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s5),
        ) {
            Eyebrow("Update needed")
            Text("This server needs a newer Iris", style = IrisType.panel, color = IrisColor.ink)
            Text(
                "Iris $installedVersion is too old for it. Open Settings and install the update: the update does not come from this server, so it works now.",
                style = IrisType.body,
                color = IrisColor.inkMuted,
            )
            ActionButton(
                "Open Settings to update",
                onOpenSettings,
                icon = Icons.Rounded.SystemUpdate,
                size = ActionSize.Large,
                modifier = Modifier.focusRequester(open),
            )
        }
        KeyHints(
            hints = listOf(KeyHint(Keys.OK, "Open Settings"), KeyHint(Keys.BACK, "Leave Iris")),
            modifier = Modifier.align(Alignment.BottomStart),
        )
    }
}
