package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.PreferencesResponse
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.PanelLabel
import studio.kahn.iris.tv.ui.components.Pill
import studio.kahn.iris.tv.ui.components.SidePanel
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.screens.home.OnboardingSave
import studio.kahn.iris.tv.ui.screens.home.OnboardingUiState
import studio.kahn.iris.tv.ui.screens.home.OnboardingViewModel
import studio.kahn.iris.tv.ui.screens.home.RowState
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * The first-run preferences sheet over the home, open while the account's preferences say
 * onboarding is not done. [onClosed] gets the server's saved preferences, or null when it
 * was put off with Back (the server is told it was skipped, as the web does).
 */
@Composable
fun OnboardingSheet(
    container: AppContainer,
    initial: PreferencesResponse,
    onClosed: (PreferencesResponse?) -> Unit,
) {
    val vm = irisViewModel(container, key = "onboarding") { c, _ -> OnboardingViewModel(c, initial) }
    val state by vm.state.collectAsStateWithLifecycle()
    LaunchedEffect(state.saved) { state.saved?.let(onClosed) }
    OnboardingContent(
        state = state,
        onToggleLanguage = vm::toggleLanguage,
        onToggleGenre = vm::toggleGenre,
        onToggleAnime = vm::toggleAnime,
        onSave = { vm.save(keep = true) },
        onSkip = { vm.save(keep = false) },
        onRetry = vm::readOptions,
        onDismiss = {
            vm.skipInBackground()
            onClosed(null)
        },
    )
}

@Composable
fun OnboardingContent(
    state: OnboardingUiState,
    onToggleLanguage: (String) -> Unit,
    onToggleGenre: (Long) -> Unit,
    onToggleAnime: () -> Unit,
    onSave: () -> Unit,
    onSkip: () -> Unit,
    onRetry: () -> Unit,
    onDismiss: () -> Unit,
) {
    val firstPill = remember { FocusRequester() }
    val save = remember { FocusRequester() }
    val languagesSettled = state.languages !is Loadable.Loading
    LaunchedEffect(languagesSettled) {
        if (!languagesSettled) return@LaunchedEffect
        val target = if (state.languages.valueOrNull.isNullOrEmpty()) save else firstPill
        runCatching { target.requestFocus() }
    }
    SidePanel(title = "Personalize your home", onDismiss = onDismiss) {
        Text(
            "Tell Iris what you are into and your suggestions follow. You can change this any time from Settings.",
            style = IrisType.body,
            color = IrisColor.inkMuted,
            modifier = Modifier.padding(horizontal = 10.dp, vertical = IrisSpace.s1),
        )
        PanelLabel("Languages")
        Hint("What you would rather watch in. Releases in these come first.")
        val languages = state.languages.valueOrNull
        if (languages == null) {
            RowState(state.languages, onRetry = onRetry, Modifier.padding(horizontal = 10.dp))
        } else {
            Pills {
                languages.forEachIndexed { i, l ->
                    Pill(
                        l.label,
                        selected = l.value in state.picks.languages,
                        onClick = { onToggleLanguage(l.value) },
                        modifier = if (i == 0) Modifier.focusRequester(firstPill) else Modifier,
                    )
                }
            }
        }
        PanelLabel("Genres")
        Hint("Pick a few you enjoy, or none for a bit of everything. Anime is its own category, apart from Animation.")
        Pills {
            Pill("Anime", selected = state.picks.includeAnime, onClick = onToggleAnime)
            state.genres.valueOrNull?.forEach { g ->
                Pill(g.name, selected = g.id in state.picks.genres, onClick = { onToggleGenre(g.id) })
            }
        }
        if (state.genres.valueOrNull == null) RowState(state.genres, onRetry = onRetry, Modifier.padding(horizontal = 10.dp))
        if (state.error != null) {
            StatusLine(
                "Not saved: ${state.error}",
                tone = StatusTone.Down,
                modifier = Modifier
                    .padding(horizontal = 10.dp, vertical = IrisSpace.s2)
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(
            Modifier.padding(start = 10.dp, end = 10.dp, top = IrisSpace.s5, bottom = IrisSpace.s3),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        ) {
            ActionButton(
                "Save preferences",
                onSave,
                busy = state.saving == OnboardingSave.Save,
                busyText = "Saving…",
                modifier = Modifier.focusRequester(save),
            )
            ActionButton(
                "Skip for now",
                onSkip,
                style = ActionStyle.Secondary,
                busy = state.saving == OnboardingSave.Skip,
                busyText = "Skipping…",
            )
        }
    }
}

@Composable
private fun Hint(text: String) {
    Text(text, style = IrisType.meta, color = IrisColor.inkMuted, modifier = Modifier.padding(horizontal = 10.dp))
}

@Composable
private fun Pills(content: @Composable () -> Unit) {
    FlowRow(
        Modifier
            .padding(horizontal = 10.dp, vertical = IrisSpace.s2)
            .focusRestorer()
            .focusGroup(),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2),
        verticalArrangement = Arrangement.spacedBy(IrisSpace.s2),
    ) { content() }
}
