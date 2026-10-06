package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Link
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.IrisWordmark
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.KeyHints
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.Spinner
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.TextInput
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.RepeatWhileStarted
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * The pairing screen, and the reference for every screen:
 *
 * 1. A stateful entry ([PairingScreen]) that owns nothing but wiring: the
 *    ViewModel from [irisViewModel], its state via
 *    `collectAsStateWithLifecycle`, work that must stop with the screen
 *    via [RepeatWhileStarted], navigation callbacks.
 * 2. A stateless body ([PairingContent]) taking the state and plain
 *    lambdas: it is what the screenshot tests render, with fake states.
 */
@Composable
fun PairingScreen(
    container: AppContainer,
    onPaired: () -> Unit,
    onUsePassword: () -> Unit,
) {
    val vm = irisViewModel(container) { c, _ -> PairingViewModel(c) }
    val state by vm.state.collectAsStateWithLifecycle()

    val polledDevice = state.code?.valueOrNull?.deviceId
    RepeatWhileStarted(polledDevice) {
        if (polledDevice != null) vm.pollUntilLinked()
    }
    LaunchedEffect(state.linked) {
        if (state.linked) onPaired()
    }

    PairingContent(
        state = state,
        onServerUrlChange = vm::onServerUrlChange,
        onGenerate = vm::generate,
        onCancel = vm::cancel,
        onUsePassword = onUsePassword,
    )
}

@Composable
fun PairingContent(
    state: PairingUiState,
    onServerUrlChange: (String) -> Unit,
    onGenerate: () -> Unit,
    onCancel: () -> Unit,
    onUsePassword: () -> Unit,
) {
    val layout = IrisLayout.current
    Box(
        Modifier
            .fillMaxSize()
            .background(IrisColor.ground)
            .padding(layout.safePadding),
    ) {
        Column(
            Modifier
                .align(Alignment.Center)
                .widthIn(max = 460.dp)
                .fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s5),
        ) {
            IrisWordmark(fontSize = 26.sp)
            val code = state.code?.valueOrNull
            if (code == null) {
                PairingForm(state, onServerUrlChange, onGenerate, onUsePassword)
            } else {
                PairingCodeView(code, state.code, onCancel)
            }
        }
        KeyHints(
            hints = listOf(KeyHint(Keys.OK, "Choose"), KeyHint(Keys.BACK, "Leave")),
            modifier = Modifier.align(Alignment.BottomStart),
        )
    }
}

@Composable
private fun PairingForm(
    state: PairingUiState,
    onServerUrlChange: (String) -> Unit,
    onGenerate: () -> Unit,
    onUsePassword: () -> Unit,
) {
    val generateFocus = remember { FocusRequester() }
    // Focus the action, not the field: focusing the field would raise the
    // keyboard before the person asked for it.
    LaunchedEffect(Unit) { runCatching { generateFocus.requestFocus() } }

    Text("Pair this TV with your Iris account", style = IrisType.panel, color = IrisColor.ink)
    Text(
        "Iris shows a code here; you confirm it on your phone or computer. No password is typed on the TV.",
        style = IrisType.body,
        color = IrisColor.inkMuted,
    )
    TextInput(
        value = state.serverUrl,
        onValueChange = onServerUrlChange,
        label = "Server address",
        leadingIcon = Icons.Rounded.Link,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
        keyboardActions = KeyboardActions(onGo = { onGenerate() }),
        modifier = Modifier.fillMaxWidth(),
    )
    val failed = state.code as? Loadable.Failed
    when {
        failed != null -> StatusLine(failed.error.message, tone = StatusTone.Down)
        state.notice != null -> StatusLine(state.notice, tone = StatusTone.Warn)
    }
    Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
        ActionButton(
            "Show a pairing code",
            onGenerate,
            enabled = state.serverUrl.isNotBlank(),
            busy = state.generating,
            busyText = "Asking the server…",
            size = ActionSize.Large,
            modifier = Modifier.focusRequester(generateFocus),
        )
        ActionButton(
            "Use email and password",
            onUsePassword,
            style = ActionStyle.Secondary,
            size = ActionSize.Large,
        )
    }
}

@Composable
private fun PairingCodeView(code: PairingCode, state: Loadable<PairingCode>?, onCancel: () -> Unit) {
    val cancelFocus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { cancelFocus.requestFocus() } }

    Text("On your phone or computer, open", style = IrisType.body, color = IrisColor.inkMuted)
    Text(code.verificationUrl, style = IrisType.metaLarge, color = IrisColor.accent)
    Text("and enter this code:", style = IrisType.body, color = IrisColor.inkMuted)
    Text(
        code.code,
        style = IrisType.hero.copy(
            fontFamily = IrisType.figure.fontFamily,
            fontWeight = FontWeight.SemiBold,
            fontFeatureSettings = IrisType.figure.fontFeatureSettings,
            letterSpacing = 0.08.em,
        ),
        color = IrisColor.ink,
    )
    if (state is Loadable.Stale) {
        StaleNotice(state.error)
    } else {
        Row(
            Modifier.semantics { liveRegion = LiveRegionMode.Polite },
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Spinner(Modifier.size(10.dp), color = IrisColor.inkMuted)
            Text("Waiting for you to confirm on the other device.", style = IrisType.meta, color = IrisColor.inkMuted)
        }
    }
    ActionButton(
        "Cancel",
        onCancel,
        style = ActionStyle.Secondary,
        modifier = Modifier.focusRequester(cancelFocus),
    )
}
