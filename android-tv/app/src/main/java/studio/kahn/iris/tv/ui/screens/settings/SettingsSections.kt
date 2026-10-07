package studio.kahn.iris.tv.ui.screens.settings

import android.os.Build
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Key
import androidx.compose.material.icons.rounded.Tv
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Icon
import androidx.tv.material3.Text
import java.time.ZonedDateTime
import studio.kahn.iris.tv.BuildConfig
import studio.kahn.iris.tv.ui.format.audioChoiceWords
import studio.kahn.iris.tv.ui.format.subtitleChoiceWords
import studio.kahn.iris.tv.data.AppUpdater
import studio.kahn.iris.tv.data.UpdateState
import studio.kahn.iris.tv.ui.update.UpdateActions
import studio.kahn.iris.tv.ui.update.UpdateButton
import studio.kahn.iris.tv.ui.update.UpdateProgress
import studio.kahn.iris.tv.data.DeviceView
import studio.kahn.iris.tv.data.InterfaceSize
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Chip
import studio.kahn.iris.tv.ui.components.ChipSize
import studio.kahn.iris.tv.ui.components.ChipTone
import studio.kahn.iris.tv.ui.components.EmptyState
import studio.kahn.iris.tv.ui.components.FactRow
import studio.kahn.iris.tv.ui.components.LoadableContent
import studio.kahn.iris.tv.ui.components.Pill
import studio.kahn.iris.tv.ui.components.Spinner
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType
import studio.kahn.iris.tv.ui.components.NoticeLine

/** Every action Settings offers, so the stateless body takes one parameter for them. */
@Immutable
class SettingsActions(
    val onOpen: (SettingsDialog) -> Unit = {},
    val onCloseDialog: () -> Unit = {},
    val onRetry: () -> Unit = {},
    val onRename: (String) -> Unit = {},
    val onUseEmailName: () -> Unit = {},
    val onSaveAudio: (String?) -> Unit = {},
    val onSaveSubtitles: (String?) -> Unit = {},
    val onToggleLanguage: (String) -> Unit = {},
    val onToggleGenre: (Long) -> Unit = {},
    val onToggleAnime: () -> Unit = {},
    val onSaveReco: () -> Unit = {},
    val onPair: (code: String, label: String) -> Unit = { _, _ -> },
    val onStopWaiting: () -> Unit = {},
    val onRevoke: (DeviceView) -> Unit = {},
    val onChangePassword: (current: String, next: String) -> Unit = { _, _ -> },
    val onSignOut: () -> Unit = {},
    val onInterfaceSize: (InterfaceSize) -> Unit = {},
    val onOpenHistory: () -> Unit = {},
    val onOpenTorrents: () -> Unit = {},
    val update: UpdateActions = UpdateActions(),
)

/** What this TV runs, for "This TV" (fixed in screenshots). */
@Immutable
data class TvFacts(val iris: String, val device: String, val android: String) {
    companion object {
        fun current() = TvFacts(
            iris = "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE}) · build ${BuildConfig.BUILD_STAMP}",
            device = "${Build.MANUFACTURER} ${Build.MODEL}",
            android = "${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})",
        )
    }
}

/** The shown [section]'s content; [now] is what dates are said against. */
@Composable
fun SettingsSectionContent(
    section: SettingsSection,
    state: SettingsUiState,
    update: UpdateState,
    actions: SettingsActions,
    now: ZonedDateTime,
    facts: TvFacts,
) {
    when (section) {
        SettingsSection.You -> YouSection(state, actions)
        SettingsSection.Playback -> PlaybackSection(state, actions)
        SettingsSection.Recommendations -> RecommendationsSection(state, actions)
        SettingsSection.Devices -> DevicesSection(state, actions, now)
        SettingsSection.Passkeys -> PasskeysSection(state, actions, now)
        SettingsSection.Password -> PasswordSection(state, actions)
        SettingsSection.App -> UpdateSection(update, actions)
        SettingsSection.ThisTv -> ThisTvSection(state, actions, facts)
    }
}

/** A read that fills the section: loading, failed with Retry, or [content]; a stale value says so. */
@Composable
private fun <T> ColumnScope.Loaded(
    value: Loadable<T>,
    onRetry: () -> Unit,
    content: @Composable ColumnScope.(T) -> Unit,
) {
    LoadableContent(value, onRetry, Modifier.heightIn(min = 120.dp)) { v ->
        Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s5)) {
            if (value is Loadable.Stale) StaleNotice(value.error)
            content(v)
        }
    }
}

@Composable
private fun YouSection(state: SettingsUiState, actions: SettingsActions) {
    SettingsGroup("You", "Shown to the household, as in “added by”. Your email stays private.") {
        Loaded(state.account, actions.onRetry) { user ->
            SettingRow("Display name", user.displayName, onClick = { actions.onOpen(SettingsDialog.Rename) })
            val fromEmail = nameFromEmail(user.email)
            if (fromEmail.isNotEmpty() && fromEmail != user.displayName) {
                ActionButton(
                    "Use “$fromEmail” from your email",
                    actions.onUseEmailName,
                    style = ActionStyle.Secondary,
                    busy = state.busy == Busy.USE_EMAIL,
                    busyText = "Saving…",
                )
            }
            NoticeLine(state.outcome.notice(SettingsSection.You))
            Column {
                FactRow("Email", user.email)
                FactRow("Role", if (user.isAdmin) "Admin" else "Member")
            }
        }
    }
}

@Composable
private fun PlaybackSection(state: SettingsUiState, actions: SettingsActions) {
    SettingsGroup("Playback", "The languages a film or an episode starts in, on every device, when it offers them.") {
        Loaded(state.playback, actions.onRetry) { prefs ->
            SettingRow("Audio", audioChoiceWords(languageChoice(prefs.audioLanguage)), onClick = { actions.onOpen(SettingsDialog.Audio) })
            SettingRow(
                "Subtitles",
                subtitleChoiceWords(languageChoice(prefs.subtitleLanguage)),
                onClick = { actions.onOpen(SettingsDialog.Subtitles) },
            )
            NoticeLine(state.outcome.notice(SettingsSection.Playback))
            Text(
                "Changing a track while watching saves the new one here too.",
                style = IrisType.meta,
                color = IrisColor.inkMuted,
            )
        }
    }
}

@Composable
private fun RecommendationsSection(state: SettingsUiState, actions: SettingsActions) {
    SettingsGroup("Recommendations", "What Home and Discover suggest first.") {
        Loaded(state.reco, actions.onRetry) { options ->
            val picks = state.draft ?: Picks.of(options.saved)
            ChoiceLabel("Languages", "What you would rather watch in. Releases in these come first.")
            if (options.languages.isEmpty()) {
                StatusLine("The languages could not be read. Try again later.", tone = StatusTone.Warn)
            } else {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                    options.languages.forEach { l ->
                        Pill(l.label, selected = l.value in picks.languages, onClick = { actions.onToggleLanguage(l.value) })
                    }
                }
            }
            ChoiceLabel(
                "Genres",
                "Pick a few you enjoy, or none for a bit of everything. Anime is its own category, apart from Animation.",
            )
            FlowRow(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                Pill("Anime", selected = picks.includeAnime, onClick = actions.onToggleAnime)
                options.genres.forEach { g ->
                    Pill(g.name, selected = g.id in picks.genres, onClick = { actions.onToggleGenre(g.id) })
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4), verticalAlignment = Alignment.CenterVertically) {
                ActionButton(
                    "Save recommendations",
                    actions.onSaveReco,
                    enabled = state.recoDirty,
                    busy = state.busy == Busy.RECO,
                    busyText = "Saving…",
                )
                Text(
                    if (state.recoDirty) "You have changes not saved yet." else "Nothing changed since the last save.",
                    style = IrisType.meta,
                    color = IrisColor.inkMuted,
                )
            }
            NoticeLine(state.outcome.notice(SettingsSection.Recommendations))
        }
    }
}

@Composable
private fun DevicesSection(state: SettingsUiState, actions: SettingsActions, now: ZonedDateTime) {
    val count = state.devices.valueOrNull?.size
    SettingsGroup(
        if (count != null && count > 0) "Devices · $count" else "Devices",
        "Pair an Android TV, or another Iris app, by entering the code it shows.",
    ) {
        val pairFocus = remember { FocusRequester() }
        var stopFocused by remember { mutableStateOf(false) }
        // "Stop waiting" goes once the wait ends: the focus it had moves to the pair button.
        LaunchedEffect(state.waitingForDevice) {
            if (!state.waitingForDevice && stopFocused) {
                stopFocused = false
                runCatching { pairFocus.requestFocus() }
            }
        }
        ActionButton(
            "Pair a TV with its code",
            { actions.onOpen(SettingsDialog.Pair) },
            icon = Icons.Rounded.Tv,
            style = ActionStyle.Secondary,
            modifier = Modifier.focusRequester(pairFocus),
        )
        if (state.waitingForDevice) {
            Row(
                Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Spinner(Modifier.size(12.dp), color = IrisColor.accent)
                Text(
                    "Code accepted. Waiting for the other TV to sign in.",
                    style = IrisType.meta,
                    color = IrisColor.ink,
                    modifier = Modifier.weight(1f, fill = false),
                )
                ActionButton(
                    "Stop waiting",
                    actions.onStopWaiting,
                    style = ActionStyle.Secondary,
                    size = ActionSize.Small,
                    modifier = Modifier.onFocusChanged { if (it.hasFocus) stopFocused = true },
                )
            }
        } else {
            NoticeLine(state.outcome.notice(SettingsSection.Devices))
        }
        Loaded(state.devices, actions.onRetry) { devices ->
            if (devices.isEmpty()) {
                Text("No paired devices yet.", style = IrisType.body, color = IrisColor.inkMuted)
            }
            devices.forEach { d ->
                DeviceRow(d, now, busy = state.busy == Busy.revoke(d), onSignOut = { actions.onOpen(SettingsDialog.Revoke(d)) })
            }
        }
    }
}

@Composable
private fun DeviceRow(device: DeviceView, now: ZonedDateTime, busy: Boolean, onSignOut: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .border(1.dp, IrisColor.line, IrisShape.card)
            .padding(horizontal = 12.dp, vertical = IrisSpace.s3),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2), verticalAlignment = Alignment.CenterVertically) {
                Text(device.name(), style = IrisType.bodyStrong, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                device.kindWords()?.let { Chip(it, size = ChipSize.Small) }
            }
            Text(device.facts(now), style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 2)
        }
        ActionButton(
            "Sign out",
            onSignOut,
            style = ActionStyle.Secondary,
            size = ActionSize.Small,
            busy = busy,
            busyText = "Signing out…",
        )
    }
}

@Composable
private fun PasskeysSection(state: SettingsUiState, actions: SettingsActions, now: ZonedDateTime) {
    val count = state.passkeys.valueOrNull?.size
    SettingsGroup(
        if (count != null && count > 0) "Passkeys · $count" else "Passkeys",
        "Optional: a passkey signs you in with your fingerprint, face or screen lock instead of your password. " +
            "Add, rename or remove them in Iris on your phone or computer; a TV cannot make one.",
    ) {
        Loaded(state.passkeys, actions.onRetry) { keys ->
            if (keys.isEmpty()) {
                EmptyState("No passkey yet", Modifier.heightIn(max = 120.dp), body = "You sign in with your password.")
            }
            keys.forEach { k ->
                ReadRow {
                    Icon(Icons.Rounded.Key, contentDescription = null, tint = IrisColor.inkMuted, modifier = Modifier.size(IrisSize.icon))
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
                        Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2), verticalAlignment = Alignment.CenterVertically) {
                            Text(k.name(), style = IrisType.bodyStrong, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            if (k.backedUp) Chip("Synced", tone = ChipTone.Ok, size = ChipSize.Small)
                        }
                        Text(k.facts(now), style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 1)
                    }
                }
            }
        }
    }
}

@Composable
private fun PasswordSection(state: SettingsUiState, actions: SettingsActions) {
    SettingsGroup("Password", "Changing it signs every device out, this TV too; each signs in again with the new one.") {
        ActionButton(
            "Change my password",
            { actions.onOpen(SettingsDialog.Password) },
            icon = Icons.Rounded.Key,
            style = ActionStyle.Secondary,
        )
        NoticeLine(state.outcome.notice(SettingsSection.Password))
    }
}

@Composable
private fun UpdateSection(update: UpdateState, actions: SettingsActions) {
    SettingsGroup(
        "App update",
        "Downloads the latest Iris from ${AppUpdater.APK_URL} and hands it to the TV’s installer, which asks you to confirm.",
    ) {
        Column {
            FactRow("Installed", "${update.installed} (${update.installedCode})")
            FactRow("Latest") { LatestLine(update) }
        }
        UpdateProgress(update.progress)
        UpdateButton(update.progress, actions.update, idle = "Download and install")
    }
}

@Composable
private fun LatestLine(update: UpdateState) {
    when (val latest = update.latest) {
        AppUpdater.VersionStatus.Unknown ->
            if (update.checking) {
                StatusLine("Checking…", tone = StatusTone.Busy)
            } else {
                StatusLine("The version check is unavailable; the download still works.", tone = StatusTone.Warn, maxLines = 2)
            }
        is AppUpdater.VersionStatus.UpToDate -> StatusLine("Up to date (latest: ${latest.latest})", tone = StatusTone.Ok)
        is AppUpdater.VersionStatus.UpdateAvailable -> StatusLine("Update available: ${latest.latest}", tone = StatusTone.Available)
    }
}

@Composable
private fun ThisTvSection(state: SettingsUiState, actions: SettingsActions, facts: TvFacts) {
    val layout = IrisLayout.current
    Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s8)) {
        InterfaceSizeGroup(state.interfaceSize, actions.onInterfaceSize)
        SettingsGroup("This TV", "What to tell whoever helps you when something goes wrong.") {
            SettingRow(
                "Server",
                state.serverUrl ?: "None",
                onClick = { actions.onOpen(SettingsDialog.ChangeServer) },
                action = "Use another",
            )
            Column {
                FactRow("Signed in as", state.account.valueOrNull?.email ?: "Unknown")
                FactRow("Iris", facts.iris)
                FactRow("Device", facts.device)
                FactRow("Android", facts.android)
                FactRow("Screen", "${layout.width.value.toInt()} × ${layout.height.value.toInt()} dp")
            }
        }
    }
}

@Composable
private fun InterfaceSizeGroup(current: InterfaceSize, onChoose: (InterfaceSize) -> Unit) {
    SettingsGroup("Interface size", "Bigger words, buttons and spacing on this TV, for a screen watched from far away. Videos keep their size.") {
        FlowRow(
            Modifier.selectableGroup(),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s2),
        ) {
            InterfaceSize.entries.forEach { size ->
                Pill(size.label, selected = size == current, onClick = { onChoose(size) }, role = Role.RadioButton)
            }
        }
    }
}
