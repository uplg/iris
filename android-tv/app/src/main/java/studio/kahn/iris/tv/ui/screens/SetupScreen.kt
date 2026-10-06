package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Link
import androidx.compose.material.icons.rounded.Mail
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
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
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.TextInput
import studio.kahn.iris.tv.ui.screens.settings.SetupUiState
import studio.kahn.iris.tv.ui.screens.settings.SetupViewModel
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * Signing in with email and password, reached from pairing ("Use email and
 * password"). Pairing by code stays the way to go: no password on the TV.
 * Back returns to pairing.
 */
@Composable
fun SetupScreen(
    container: AppContainer,
    onAuthenticated: () -> Unit,
    onUseCode: () -> Unit = {},
) {
    val vm = irisViewModel(container) { c, _ -> SetupViewModel(c) }
    val state by vm.state.collectAsStateWithLifecycle()
    LaunchedEffect(state.signedIn) { if (state.signedIn) onAuthenticated() }
    SetupContent(
        state = state,
        onServerUrlChange = vm::onServerUrlChange,
        onEmailChange = vm::onEmailChange,
        onPasswordChange = vm::onPasswordChange,
        onSignIn = vm::signIn,
        onUseCode = onUseCode,
    )
}

@Composable
fun SetupContent(
    state: SetupUiState,
    onServerUrlChange: (String) -> Unit,
    onEmailChange: (String) -> Unit,
    onPasswordChange: (String) -> Unit,
    onSignIn: () -> Unit,
    onUseCode: () -> Unit,
) {
    val layout = IrisLayout.current
    val emailFocus = remember { FocusRequester() }
    val focus = LocalFocusManager.current
    // The person came here to type: the email field is where they start.
    LaunchedEffect(Unit) { runCatching { emailFocus.requestFocus() } }
    val next = KeyboardActions(onNext = { focus.moveFocus(FocusDirection.Down) })
    Column(
        Modifier
            .fillMaxSize()
            .background(IrisColor.ground)
            .padding(layout.safePadding),
        verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
    ) {
        Box(
            Modifier
                .weight(1f)
                .fillMaxWidth(),
            contentAlignment = Alignment.Center,
        ) {
            Column(
                Modifier
                    .widthIn(max = 460.dp)
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(IrisSpace.s5),
            ) {
                IrisWordmark(fontSize = 26.sp)
                Text("Sign in with email and password", style = IrisType.title, color = IrisColor.ink)
                Text(
                    "The password is typed on the TV. Pairing with a code shown here avoids it.",
                    style = IrisType.body,
                    color = IrisColor.inkMuted,
                )
                TextInput(
                    value = state.serverUrl,
                    onValueChange = onServerUrlChange,
                    label = "Server address",
                    leadingIcon = Icons.Rounded.Link,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
                    keyboardActions = next,
                    modifier = Modifier.fillMaxWidth(),
                )
                TextInput(
                    value = state.email,
                    onValueChange = onEmailChange,
                    label = "Email",
                    leadingIcon = Icons.Rounded.Mail,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email, imeAction = ImeAction.Next),
                    keyboardActions = next,
                    modifier = Modifier
                        .fillMaxWidth()
                        .focusRequester(emailFocus),
                )
                TextInput(
                    value = state.password,
                    onValueChange = onPasswordChange,
                    label = "Password",
                    masked = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { onSignIn() }),
                    modifier = Modifier.fillMaxWidth(),
                )
                if (state.error != null) {
                    StatusLine(state.error, tone = StatusTone.Down, modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite })
                }
                Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
                    ActionButton(
                        "Sign in",
                        onSignIn,
                        enabled = state.canSignIn,
                        busy = state.signingIn,
                        busyText = "Signing in…",
                        size = ActionSize.Large,
                    )
                    ActionButton("Pair with a code instead", onUseCode, style = ActionStyle.Secondary, size = ActionSize.Large)
                }
            }
        }
        KeyHints(hints = listOf(KeyHint(Keys.OK, "Choose"), KeyHint(Keys.BACK, "Back to pairing")))
    }
}
