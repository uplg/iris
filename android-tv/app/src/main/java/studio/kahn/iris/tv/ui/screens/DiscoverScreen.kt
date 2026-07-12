package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.tv.material3.ExperimentalTvMaterial3Api
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.Spacing

/**
 * The single "Discover" destination: both halves of the recommendation
 * system as TABS — "For You" (organized taste shelves) and "Tonight"
 * (mood board). They always were one system fed by the same engine;
 * exposing them as two separate nav items read as two products, and
 * nobody knew which door to take. One icon, one page, two tabs.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun DiscoverScreen(
    container: AppContainer,
    onOpenCollection: (String) -> Unit,
    onPickResult: (providerId: String, externalId: String, tmdbId: Long?, kind: String?) -> Unit,
    onOpenSearch: (String) -> Unit,
) {
    // Saveable so Back-from-detail lands on the tab the user was
    // browsing, not back on For You.
    var tab by rememberSaveable { mutableIntStateOf(0) }
    Column(Modifier.fillMaxSize().background(IrisColors.Background)) {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = Spacing.gutter)
                .padding(top = Spacing.xl),
            horizontalArrangement = Arrangement.spacedBy(Spacing.md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Eyebrow("Discover")
            IrisButton(
                text = "For You",
                onClick = { tab = 0 },
                variant = if (tab == 0) IrisButtonVariant.Primary else IrisButtonVariant.Ghost,
            )
            IrisButton(
                text = "Tonight",
                onClick = { tab = 1 },
                variant = if (tab == 1) IrisButtonVariant.Primary else IrisButtonVariant.Ghost,
            )
        }
        Box(Modifier.fillMaxWidth().weight(1f)) {
            if (tab == 0) {
                ForYouScreen(
                    container = container,
                    onOpenCollection = onOpenCollection,
                    onPickResult = onPickResult,
                    onOpenSearch = onOpenSearch,
                )
            } else {
                MoodsScreen(
                    container = container,
                    onOpenCollection = onOpenCollection,
                    onPickResult = onPickResult,
                    onOpenSearch = onOpenSearch,
                )
            }
        }
    }
}
