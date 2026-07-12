package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Border
import androidx.tv.material3.ClickableSurfaceDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.Text
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing
import studio.kahn.iris.tv.ui.theme.irisAmbient

/**
 * The single "Discover" destination: both halves of the recommendation
 * system as TABS — "For You" (organized taste shelves) and "Tonight"
 * (mood board). They always were one system fed by the same engine;
 * exposing them as two separate nav items read as two products, and
 * nobody knew which door to take. One icon, one page, two tabs.
 *
 * The Background + ambient gradient live HERE, once, for the whole
 * screen — the hosted tab contents must not repaint their own opaque
 * backgrounds or the tab strip sits on a flat black band above them.
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
    Box(Modifier.fillMaxSize().background(IrisColors.Background)) {
        Box(Modifier.fillMaxSize().background(irisAmbient()))
        Column(Modifier.fillMaxSize()) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = Spacing.gutter)
                    .padding(top = Spacing.xl),
                horizontalArrangement = Arrangement.spacedBy(Spacing.md),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Eyebrow("Discover")
                DiscoverTab("For You", selected = tab == 0) { tab = 0 }
                DiscoverTab("Tonight", selected = tab == 1) { tab = 1 }
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
}

/** Season-tabs-style pill (no hairline border — a bordered Ghost button
 *  here read as a black box): selected = elevated fill, unselected =
 *  faint overlay, focus = the brand ring. */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun DiscoverTab(label: String, selected: Boolean, onClick: () -> Unit) {
    val pill = RoundedCornerShape(Radius.pill)
    Surface(
        onClick = onClick,
        shape = ClickableSurfaceDefaults.shape(shape = pill),
        scale = ClickableSurfaceDefaults.scale(focusedScale = 1f),
        colors = ClickableSurfaceDefaults.colors(
            containerColor = if (selected) IrisColors.Elev2 else IrisColors.Overlay06,
            contentColor = if (selected) IrisColors.Foreground else IrisColors.MutedForeground,
            focusedContainerColor = if (selected) IrisColors.Elev2 else IrisColors.Overlay12,
            focusedContentColor = IrisColors.Foreground,
        ),
        border = ClickableSurfaceDefaults.border(
            border = Border.None,
            focusedBorder = Border(BorderStroke(Focus.ring, IrisColors.Brand), shape = pill),
        ),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.titleSmall,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
        )
    }
}
