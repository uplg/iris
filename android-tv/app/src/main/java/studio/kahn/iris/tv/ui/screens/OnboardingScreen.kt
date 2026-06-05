package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Border
import androidx.tv.material3.ClickableSurfaceDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.Icon
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.Text
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.GenreOption
import studio.kahn.iris.tv.data.LanguageOption
import studio.kahn.iris.tv.data.Preferences
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing
import studio.kahn.iris.tv.ui.theme.irisAmbient

/**
 * First-run onboarding (full-screen). Shown by [HomeScreen] when the
 * user's preferences exist but `onboarding_completed` is false.
 *
 * Languages + genres are picked as D-pad chips. **Anime is its own chip**
 * — a distinct category, NOT TMDB's "Animation" genre — driven by the
 * separate `include_anime` preference (the AniList pipeline backs it in a
 * later slice). Both actions persist `onboarding_completed = true`, so the
 * screen never reappears; [onDone] returns the user to Home.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun OnboardingScreen(
    container: AppContainer,
    initialPrefs: Preferences,
    onDone: () -> Unit,
) {
    val scope = rememberCoroutineScope()

    var languages by remember { mutableStateOf(initialPrefs.languages) }
    var genres by remember { mutableStateOf(initialPrefs.genres) }
    var includeAnime by remember { mutableStateOf(initialPrefs.includeAnime) }
    var languageOptions by remember { mutableStateOf<List<LanguageOption>>(emptyList()) }
    var genreOptions by remember { mutableStateOf<List<GenreOption>>(emptyList()) }
    var languagesLoading by remember { mutableStateOf(true) }
    var genresLoading by remember { mutableStateOf(true) }
    var saving by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        val url = container.sessionStore.serverUrl.first()
        if (url != null) {
            val api = container.apiFor(url)
            val (langs, gen) = withContext(Dispatchers.IO) {
                val l = runCatching { api.languages() }.getOrNull()
                val g = runCatching { api.genres() }.getOrNull()
                l to g
            }
            languageOptions = langs?.languages.orEmpty()
            genreOptions = gen?.genres.orEmpty()
        }
        languagesLoading = false
        genresLoading = false
    }

    // Initial D-pad focus on the first language chip — fired once the
    // (server-driven) language list has loaded so the requester is
    // actually attached.
    val firstFocus = remember { FocusRequester() }
    LaunchedEffect(languageOptions) {
        if (languageOptions.isNotEmpty()) {
            runCatching { firstFocus.requestFocus() }
        }
    }

    // `keep` distinguishes "Save preferences" (persist selections) from
    // "Skip for now" (clear them — cold-start fallback). Both complete
    // onboarding.
    fun finish(keep: Boolean) {
        if (saving) return
        saving = true
        scope.launch {
            val url = container.sessionStore.serverUrl.first()
            if (url != null) {
                val body = Preferences(
                    languages = if (keep) languages else emptyList(),
                    genres = if (keep) genres else emptyList(),
                    includeAnime = if (keep) includeAnime else false,
                    onboardingCompleted = true,
                )
                withContext(Dispatchers.IO) {
                    runCatching { container.apiFor(url).savePreferences(body) }
                }
            }
            onDone()
        }
    }

    Box(Modifier.fillMaxSize().background(IrisColors.Background)) {
        Box(Modifier.fillMaxSize().background(irisAmbient()))

        LazyColumn(
            modifier = Modifier.fillMaxSize().widthIn(max = 1100.dp),
            contentPadding = PaddingValues(vertical = Spacing.xxxl),
            verticalArrangement = Arrangement.spacedBy(Spacing.xl),
        ) {
            item(key = "header") {
                Column(
                    modifier = Modifier.padding(horizontal = Spacing.gutter),
                    verticalArrangement = Arrangement.spacedBy(Spacing.sm),
                ) {
                    Eyebrow("Personalize", color = IrisColors.Brand)
                    SectionTitle("Make Iris yours")
                    Text(
                        "Tell us what you're into and we'll tune your recommendations. " +
                            "You can change this anytime in Settings.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = IrisColors.FgDim,
                    )
                }
            }

            item(key = "languages") {
                Column(
                    modifier = Modifier.padding(horizontal = Spacing.gutter),
                    verticalArrangement = Arrangement.spacedBy(Spacing.md),
                ) {
                    Eyebrow("Languages")
                    if (languagesLoading) {
                        Text(
                            "Loading languages…",
                            style = MaterialTheme.typography.bodyMedium,
                            color = IrisColors.FgDim,
                        )
                    } else {
                        Row(horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
                            languageOptions.forEachIndexed { idx, option ->
                                val value = option.value
                                SelectableChip(
                                    label = option.label,
                                    selected = languages.contains(value),
                                    onClick = {
                                        languages =
                                            if (languages.contains(value)) languages - value
                                            else languages + value
                                    },
                                    modifier = if (idx == 0) Modifier.focusRequester(firstFocus) else Modifier,
                                )
                            }
                        }
                    }
                }
            }

            item(key = "genres-label") {
                Eyebrow(
                    "Genres",
                    modifier = Modifier.padding(horizontal = Spacing.gutter),
                )
            }
            item(key = "genres-row") {
                LazyRow(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(Spacing.sm),
                    contentPadding = PaddingValues(horizontal = Spacing.gutter),
                ) {
                    // Anime is a distinct category (NOT TMDB's "Animation")
                    // and always selectable, even before the TMDB genres
                    // load.
                    item(key = "anime") {
                        SelectableChip(
                            label = "Anime",
                            selected = includeAnime,
                            onClick = { includeAnime = !includeAnime },
                            accent = true,
                        )
                    }
                    if (genresLoading) {
                        item(key = "genres-loading") {
                            Text(
                                "Loading genres…",
                                style = MaterialTheme.typography.bodyMedium,
                                color = IrisColors.FgDim,
                                modifier = Modifier.padding(vertical = Spacing.md),
                            )
                        }
                    } else {
                        items(genreOptions, key = { it.id }) { g ->
                            SelectableChip(
                                label = g.name,
                                selected = genres.contains(g.id),
                                onClick = {
                                    genres =
                                        if (genres.contains(g.id)) genres - g.id
                                        else genres + g.id
                                },
                            )
                        }
                    }
                }
            }

            item(key = "actions") {
                Row(
                    modifier = Modifier.padding(horizontal = Spacing.gutter, vertical = Spacing.md),
                    horizontalArrangement = Arrangement.spacedBy(Spacing.md),
                ) {
                    IrisButton(
                        text = "Skip for now",
                        onClick = { finish(false) },
                        variant = IrisButtonVariant.Ghost,
                        enabled = !saving,
                    )
                    IrisButton(
                        text = if (saving) "Saving…" else "Save preferences",
                        onClick = { finish(true) },
                        enabled = !saving,
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SelectableChip(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    accent: Boolean = false,
) {
    val shape = RoundedCornerShape(Radius.pill)
    val resting = if (selected) IrisColors.BrandSoft else IrisColors.Overlay06
    val focused = if (selected) IrisColors.BrandSoft else IrisColors.Overlay12
    val restingBorder = when {
        selected -> IrisColors.Brand
        accent -> IrisColors.Brand.copy(alpha = 0.4f)
        else -> IrisColors.Border
    }
    Surface(
        onClick = onClick,
        modifier = modifier,
        shape = ClickableSurfaceDefaults.shape(shape),
        scale = ClickableSurfaceDefaults.scale(focusedScale = Focus.controlScale),
        colors = ClickableSurfaceDefaults.colors(
            containerColor = resting,
            contentColor = IrisColors.Foreground,
            focusedContainerColor = focused,
            focusedContentColor = IrisColors.Foreground,
            pressedContainerColor = focused,
            pressedContentColor = IrisColors.Foreground,
        ),
        border = ClickableSurfaceDefaults.border(
            border = Border(BorderStroke(1.dp, restingBorder), shape = shape),
            focusedBorder = Border(BorderStroke(Focus.ring, IrisColors.Brand), shape = shape),
        ),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 18.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(Spacing.sm),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (selected) {
                Icon(imageVector = Icons.Filled.Check, contentDescription = null)
            }
            Text(label, style = MaterialTheme.typography.titleSmall)
        }
    }
}
