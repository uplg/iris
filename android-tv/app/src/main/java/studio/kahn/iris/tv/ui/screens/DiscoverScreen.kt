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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
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
import studio.kahn.iris.tv.ui.components.TvIconButton
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing
import studio.kahn.iris.tv.ui.theme.irisAmbient
import studio.kahn.iris.tv.ui.components.touchClick

/**
 * The single "Discover" destination — both halves of the reco system
 * as tabs ("For You" + "Tonight"). One icon, one page, two tabs.
 * Background + ambient are painted HERE, once: hosted tab contents
 * must not repaint their own opaque background or the tab strip
 * sits on a flat black band.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun DiscoverScreen(
    container: AppContainer,
    onOpenCollection: (String) -> Unit,
    onPickResult: (providerId: String, externalId: String, tmdbId: Long?, kind: String?) -> Unit,
    onOpenSearch: (String) -> Unit,
    onBack: () -> Unit,
) {
    // Saveable: Back-from-detail lands on the tab the user was on.
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
                // Visible Back — on TV the remote covers it, but on a
                // phone the only exit was a system gesture.
                TvIconButton(
                    icon = Icons.AutoMirrored.Filled.ArrowBack,
                    contentDescription = "Back",
                    onClick = onBack,
                )
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

/** Season-tabs-style pill — a bordered Ghost button here read as
 *  a black box. */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun DiscoverTab(label: String, selected: Boolean, onClick: () -> Unit) {
    val pill = RoundedCornerShape(Radius.pill)
    Surface(
        onClick = onClick,
        modifier = Modifier.touchClick(onClick = onClick),
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
