package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSpace

/**
 * A screen with a footer at its bottom edge (a [ScreenFooter]: the key hints, a notice above
 * them). [content] takes the whole screen, drawn over the footer so its dialogs and side
 * panels cover the hints, and is handed the footer's height: what it lays out or scrolls
 * stops that far from the bottom (`Modifier.padding(bottom = footer)`), so nothing sits or
 * scrolls under the hints.
 */
@Composable
fun FooterLayout(
    footer: @Composable () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable (footer: Dp) -> Unit,
) {
    SubcomposeLayout(modifier) { constraints ->
        val loose = constraints.copy(minWidth = 0, minHeight = 0)
        val bottom = subcompose(FooterSlot.Footer) {
            Box(Modifier.fillMaxWidth()) { footer() }
        }.map { it.measure(loose) }
        val height = bottom.maxOfOrNull { it.height } ?: 0
        val body = subcompose(FooterSlot.Content) { content(height.toDp()) }.map { it.measure(constraints) }
        val width = constraints.maxWidth
        val screen = constraints.maxHeight
        layout(width, screen) {
            bottom.forEach { it.place(0, screen - it.height) }
            body.forEach { it.place(0, 0) }
        }
    }
}

private enum class FooterSlot { Content, Footer }

/**
 * The footer every board ends with: [above] (a notice, the action's outcome), then the
 * [KeyHints], 15 dp above the bottom edge inside the safe margins. [framed]: the library's
 * full-width band instead (TVLibrary, the search results).
 */
@Composable
fun ScreenFooter(
    hints: List<KeyHint>,
    modifier: Modifier = Modifier,
    trailing: String? = null,
    framed: Boolean = false,
    above: @Composable ColumnScope.() -> Unit = {},
) {
    val layout = IrisLayout.current
    if (framed) {
        Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
            Column(Modifier.padding(horizontal = layout.safeHorizontal), content = above)
            KeyHints(hints, trailing = trailing, framed = true)
        }
    } else {
        Column(
            modifier
                .fillMaxWidth()
                .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, top = IrisSpace.s2, bottom = FOOTER_BOTTOM),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        ) {
            above()
            KeyHints(hints, trailing = trailing)
        }
    }
}

private val FOOTER_BOTTOM = 15.dp
