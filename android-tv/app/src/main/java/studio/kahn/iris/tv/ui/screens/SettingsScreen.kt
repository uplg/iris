package studio.kahn.iris.tv.ui.screens

import java.time.ZonedDateTime
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Logout
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.ConfirmDialog
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.KeyHints
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.PanelOption
import studio.kahn.iris.tv.ui.components.SidePanel
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.TextInput
import studio.kahn.iris.tv.ui.screens.settings.Busy
import studio.kahn.iris.tv.ui.screens.settings.DialogField
import studio.kahn.iris.tv.ui.screens.settings.FormDialog
import studio.kahn.iris.tv.ui.screens.settings.PASSWORD_MIN
import studio.kahn.iris.tv.ui.screens.settings.RailItem
import studio.kahn.iris.tv.ui.screens.settings.SettingsActions
import studio.kahn.iris.tv.ui.screens.settings.SettingsDialog
import studio.kahn.iris.tv.ui.screens.settings.SettingsSection
import studio.kahn.iris.tv.ui.screens.settings.SettingsSectionContent
import studio.kahn.iris.tv.ui.screens.settings.SettingsUiState
import studio.kahn.iris.tv.ui.screens.settings.SettingsViewModel
import studio.kahn.iris.tv.ui.screens.settings.TvFacts
import studio.kahn.iris.tv.data.UpdateState
import studio.kahn.iris.tv.ui.update.actions
import studio.kahn.iris.tv.ui.format.audioChoiceWords
import studio.kahn.iris.tv.ui.screens.settings.languageChoice
import studio.kahn.iris.tv.ui.format.NO_SUBTITLES
import studio.kahn.iris.tv.ui.screens.settings.languageOptions
import studio.kahn.iris.tv.ui.screens.settings.name
import studio.kahn.iris.tv.ui.format.subtitleChoiceWords
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.state.RepeatWhileStarted
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType
import studio.kahn.iris.tv.ui.components.PanelOptions

/**
 * Settings and the account: the web's account page (who I am, playback
 * languages, recommendations, devices, passkeys read only, password) plus
 * what only a TV has (its server, the app update, what it runs on).
 * Reached from the account in the header; the only screen left open when
 * the server refuses this app as too old (it holds the updater).
 *
 * [initialSection] is shown first ([SettingsSection.App] when the update
 * lock sent the person here). [onAccountChanged] tells the header the
 * display name changed.
 */
@Composable
fun SettingsScreen(
    container: AppContainer,
    onOpenHistory: () -> Unit,
    onOpenTorrents: () -> Unit,
    onSignOut: () -> Unit,
    onBack: () -> Unit,
    onAccountChanged: () -> Unit = {},
    initialSection: SettingsSection = SettingsSection.You,
) {
    val vm = irisViewModel(container) { c, _ -> SettingsViewModel(c) }
    val state by vm.state.collectAsStateWithLifecycle()
    // The wait for a paired TV reads only while Settings is in front.
    RepeatWhileStarted(state.waitingForDevice) { if (state.waitingForDevice) vm.waitForDeviceWhileStarted() }
    val update by container.updates.state.collectAsStateWithLifecycle()
    LaunchedEffect(Unit) { container.updates.check(force = true) }

    LaunchedEffect(state.signedOut) { if (state.signedOut) onSignOut() }
    LaunchedEffect(state.nameChanges) { if (state.nameChanges > 0) onAccountChanged() }

    SettingsContent(
        state = state,
        update = update,
        initialSection = initialSection,
        onBack = onBack,
        actions = SettingsActions(
            onOpen = vm::open,
            onCloseDialog = vm::closeDialog,
            onRetry = vm::refresh,
            onRename = vm::rename,
            onUseEmailName = vm::useEmailName,
            onSaveAudio = vm::saveAudio,
            onSaveSubtitles = vm::saveSubtitles,
            onToggleLanguage = vm::toggleLanguage,
            onToggleGenre = vm::toggleGenre,
            onToggleAnime = vm::toggleAnime,
            onSaveReco = vm::saveReco,
            onPair = vm::pair,
            onStopWaiting = vm::stopWaiting,
            onRevoke = vm::revoke,
            onChangePassword = vm::changePassword,
            onSignOut = vm::signOut,
            onOpenHistory = onOpenHistory,
            onOpenTorrents = onOpenTorrents,
            update = container.updates.actions(),
        ),
    )
}

/**
 * The stateless body: a page title with the account's words and the
 * history, torrents and sign-out actions; the section rail on the left
 * (moving onto a section shows it); the section on the right. Back from a
 * section goes to the rail, Back from the rail leaves ([onBack]).
 */
@Composable
fun SettingsContent(
    state: SettingsUiState,
    update: UpdateState,
    actions: SettingsActions,
    onBack: () -> Unit = {},
    initialSection: SettingsSection = SettingsSection.You,
    now: ZonedDateTime = ZonedDateTime.now(),
    facts: TvFacts = remember { TvFacts.current() },
) {
    val layout = IrisLayout.current
    var section by rememberSaveable { mutableStateOf(initialSection) }
    val railFocus = remember { FocusRequester() }
    var railFocused by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { runCatching { railFocus.requestFocus() } }
    val sectionFocus = remember { FocusRequester() }
    var dialogWasOpen by remember { mutableStateOf(false) }
    LaunchedEffect(state.dialog == null) {
        if (state.dialog != null) {
            dialogWasOpen = true
        } else if (dialogWasOpen) {
            dialogWasOpen = false
            withFrameNanos { }
            runCatching { sectionFocus.requestFocus() }
        }
    }
    BackHandler(enabled = state.dialog == null) {
        if (railFocused) onBack() else railFocus.requestFocus()
    }

    Box(Modifier.fillMaxSize()) {
        Column(
            Modifier
                .fillMaxSize()
                .padding(layout.safePadding),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s6),
        ) {
            PageHead(state, actions)
            Row(Modifier.weight(1f)) {
                Column(
                    Modifier
                        .width(RAIL_WIDTH)
                        .selectableGroup()
                        .onFocusChanged { railFocused = it.hasFocus }
                        .focusRestorer(railFocus)
                        .focusGroup()
                        .verticalScroll(rememberScrollState())
                        .padding(IrisSpace.s1),
                    verticalArrangement = Arrangement.spacedBy(IrisSpace.s1),
                ) {
                    SettingsSection.entries.forEach { s ->
                        RailItem(
                            if (s == SettingsSection.App && update.available != null) "App update · new" else s.label,
                            selected = s == section,
                            onClick = { section = s },
                            modifier = Modifier
                                .then(if (s == section) Modifier.focusRequester(railFocus) else Modifier)
                                .onFocusChanged { if (it.isFocused) section = s },
                        )
                    }
                }
                Spacer(Modifier.width(IrisSpace.s9))
                key(section) {
                    Column(
                        Modifier
                            .weight(1f)
                            // A dialog or a panel closing gives the focus back to its opener.
                            .focusRequester(sectionFocus)
                            .focusRestorer()
                            .focusGroup()
                            .verticalScroll(rememberScrollState())
                            .padding(IrisSpace.s1),
                        verticalArrangement = Arrangement.spacedBy(IrisSpace.s5),
                    ) {
                        SettingsSectionContent(section, state, update, actions, now, facts)
                    }
                }
            }
            KeyHints(
                hints = listOf(
                    KeyHint(Keys.OK, "Choose"),
                    KeyHint(Keys.BACK, if (railFocused) "Leave Settings" else "To the sections"),
                ),
            )
        }
        SettingsDialogs(state, actions)
    }
}

private val RAIL_WIDTH = 170.dp

@Composable
private fun PageHead(state: SettingsUiState, actions: SettingsActions) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
            Text("Settings", style = IrisType.page, color = IrisColor.ink)
            val user = state.account.valueOrNull
            if (user != null) {
                Text("${user.displayName} · ${user.email}", style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 1)
            }
        }
        ActionButton("Watch history", actions.onOpenHistory, style = ActionStyle.Secondary, size = ActionSize.Small)
        ActionButton("Torrents", actions.onOpenTorrents, style = ActionStyle.Secondary, size = ActionSize.Small)
        ActionButton(
            "Sign out",
            actions.onSignOut,
            icon = Icons.AutoMirrored.Rounded.Logout,
            style = ActionStyle.Secondary,
            size = ActionSize.Small,
            busy = state.busy == Busy.SIGN_OUT,
            busyText = "Signing out…",
        )
    }
}

@Composable
private fun SettingsDialogs(state: SettingsUiState, actions: SettingsActions) {
    val error = state.dialogError
    when (val dialog = state.dialog) {
        null -> Unit
        SettingsDialog.Rename -> {
            val current = state.account.valueOrNull?.displayName.orEmpty()
            var name by remember(current) { mutableStateOf(current) }
            val first = remember { FocusRequester() }
            FormDialog(
                eyebrow = "You",
                title = "Your display name",
                body = "Shown to the household, as in “added by”.",
                confirmLabel = "Save the name",
                onConfirm = { actions.onRename(name) },
                onCancel = actions.onCloseDialog,
                firstFocus = first,
                busy = state.busy == Busy.RENAME,
                busyText = "Saving…",
            ) {
                TextInput(
                    name,
                    { name = it },
                    label = "Display name",
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Words, imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { actions.onRename(name) }),
                    modifier = Modifier
                        .fillMaxWidth()
                        .focusRequester(first),
                )
                if (error != null) StatusLine(error.text, tone = StatusTone.Down)
            }
        }
        SettingsDialog.Password -> {
            var current by remember { mutableStateOf("") }
            var next by remember { mutableStateOf("") }
            val first = remember { FocusRequester() }
            val focus = LocalFocusManager.current
            FormDialog(
                eyebrow = "Password",
                title = "Change your password",
                body = "Every device signs out, this TV too; each signs in again with the new one.",
                confirmLabel = "Change my password",
                onConfirm = { actions.onChangePassword(current, next) },
                onCancel = actions.onCloseDialog,
                firstFocus = first,
                busy = state.busy == Busy.PASSWORD,
                busyText = "Changing…",
            ) {
                TextInput(
                    current,
                    { current = it },
                    label = "Current password",
                    masked = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                    keyboardActions = KeyboardActions(onNext = { focus.moveFocus(FocusDirection.Down) }),
                    modifier = Modifier
                        .fillMaxWidth()
                        .focusRequester(first),
                )
                if (error != null && error.field == DialogField.First) StatusLine(error.text, tone = StatusTone.Down)
                TextInput(
                    next,
                    { next = it },
                    label = "New password, $PASSWORD_MIN characters or more",
                    masked = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { actions.onChangePassword(current, next) }),
                    modifier = Modifier.fillMaxWidth(),
                )
                if (error != null && error.field == DialogField.Second) StatusLine(error.text, tone = StatusTone.Down)
            }
        }
        SettingsDialog.Pair -> {
            var code by remember { mutableStateOf("") }
            var label by remember { mutableStateOf("") }
            val first = remember { FocusRequester() }
            val focus = LocalFocusManager.current
            FormDialog(
                eyebrow = "Devices",
                title = "Pair a TV with its code",
                body = "On the other TV, choose Show a pairing code, then enter the code it shows.",
                confirmLabel = "Pair the TV",
                onConfirm = { actions.onPair(code, label) },
                onCancel = actions.onCloseDialog,
                firstFocus = first,
                busy = state.busy == Busy.PAIR,
                busyText = "Pairing…",
            ) {
                TextInput(
                    code,
                    { code = it.uppercase() },
                    label = "Pairing code, like WX7K-ABCD",
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Characters, imeAction = ImeAction.Next),
                    keyboardActions = KeyboardActions(onNext = { focus.moveFocus(FocusDirection.Down) }),
                    modifier = Modifier
                        .fillMaxWidth()
                        .focusRequester(first),
                )
                if (error != null) StatusLine(error.text, tone = StatusTone.Down)
                TextInput(
                    label,
                    { label = it.take(LABEL_MAX) },
                    label = "Device name (optional), like Living room TV",
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { actions.onPair(code, label) }),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
        SettingsDialog.Audio -> LanguagePanel(
            title = "Audio language",
            current = languageChoice(state.playback.valueOrNull?.audioLanguage),
            withOff = false,
            words = ::audioChoiceWords,
            error = error?.text,
            onPick = actions.onSaveAudio,
            onDismiss = actions.onCloseDialog,
        )
        SettingsDialog.Subtitles -> LanguagePanel(
            title = "Subtitle language",
            current = languageChoice(state.playback.valueOrNull?.subtitleLanguage),
            withOff = true,
            words = ::subtitleChoiceWords,
            error = error?.text,
            onPick = actions.onSaveSubtitles,
            onDismiss = actions.onCloseDialog,
        )
        is SettingsDialog.Revoke -> ConfirmDialog(
            eyebrow = "Devices",
            title = "Sign out ${dialog.device.name()}?",
            body = "It stops playing from Iris at once. To use it again, pair it with a new code. If it is this TV, it asks to be paired again.",
            confirmLabel = "Sign the device out",
            onConfirm = { actions.onRevoke(dialog.device) },
            onCancel = actions.onCloseDialog,
        )
        SettingsDialog.ChangeServer -> ConfirmDialog(
            eyebrow = "This TV",
            title = "Use another Iris server?",
            body = "This TV signs out of ${state.serverUrl ?: "its server"}. Then enter the other address on the pairing screen.",
            confirmLabel = "Sign out and change",
            onConfirm = {
                actions.onCloseDialog()
                actions.onSignOut()
            },
            onCancel = actions.onCloseDialog,
        )
    }
}

private const val LABEL_MAX = 64

/** The language choices of a [SidePanel], the current one ticked and focused; picking saves it. */
@Composable
private fun LanguagePanel(
    title: String,
    current: String?,
    withOff: Boolean,
    words: (String?) -> String,
    error: String?,
    onPick: (String?) -> Unit,
    onDismiss: () -> Unit,
) {
    val options: List<String?> = buildList {
        add(null)
        if (withOff) add(NO_SUBTITLES)
        addAll(languageOptions(current))
    }
    SidePanel(title = title, onDismiss = onDismiss, footer = "Saved for every device at once.") {
        if (error != null) StatusLine(error, tone = StatusTone.Down, modifier = Modifier.padding(horizontal = 10.dp))
        PanelOptions(options, selected = current, onSelect = onPick, label = words, focusOnOpen = true)
    }
}
