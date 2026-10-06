package studio.kahn.iris.tv.ui.components

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import studio.kahn.iris.tv.ui.theme.IrisFocus
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * The right-hand sheet (TVPlayerTracks): [title] in Fraunces, then [content]
 * (typically [PanelLabel]s and [PanelOption]s), scrolling when long, and an
 * optional muted [footer] at the bottom. Back closes it ([onDismiss]), so
 * does a tap beside it on a phone. The D-pad cannot leave it while it is
 * open. Focus a first control yourself when it opens (the selected option).
 * Render it last inside a full-screen Box so it draws on top.
 */
@Composable
fun SidePanel(
    title: String,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    footer: String? = null,
    eyebrow: String? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    BackHandler(onBack = onDismiss)
    val layout = IrisLayout.current
    Row(modifier.fillMaxSize()) {
        Spacer(
            Modifier
                .weight(1f)
                .fillMaxHeight()
                .touchClick(onClick = onDismiss),
        )
        Column(
            Modifier
                .width(minOf(IrisSize.sidePanel, layout.width * 0.6f))
                .fillMaxHeight()
                .background(IrisColor.surface)
                .drawBehind {
                    drawLine(IrisColor.line, Offset(0.5.dp.toPx(), 0f), Offset(0.5.dp.toPx(), size.height), strokeWidth = 1.dp.toPx())
                }
                .semantics { paneTitle = title }
                .focusProperties { onExit = { cancelFocusChange() } }
                .focusGroup()
                .padding(start = 24.dp, end = 24.dp, top = layout.safeVertical, bottom = 20.dp),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s1),
        ) {
            if (eyebrow != null) Eyebrow(eyebrow, Modifier.padding(horizontal = 10.dp))
            Text(
                title,
                style = IrisType.panel,
                color = IrisColor.ink,
                modifier = Modifier.padding(start = 10.dp, end = 10.dp, bottom = IrisSpace.s1),
            )
            Column(
                Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    // The scroll clips: room for the focus ring of the first and last options.
                    .padding(vertical = IrisFocus.ringWidth + IrisFocus.ringOffset),
                verticalArrangement = Arrangement.spacedBy(IrisSpace.s1),
                content = content,
            )
            if (footer != null) {
                Text(footer, style = IrisType.meta, color = IrisColor.inkMuted, modifier = Modifier.padding(horizontal = 10.dp))
            }
        }
    }
}

/** A group label inside a [SidePanel] ("AUDIO", "SUBTITLES"). */
@Composable
fun PanelLabel(text: String, modifier: Modifier = Modifier) {
    Text(
        text.uppercase(),
        style = IrisType.eyebrow.copy(fontSize = IrisType.metaSmall.fontSize),
        color = IrisColor.inkMuted,
        modifier = modifier.padding(start = 10.dp, end = 10.dp, top = 8.dp, bottom = 4.dp),
    )
}

/**
 * One radio option of a [SidePanel] group: a ring that fills when chosen
 * (shape, not only color), accent words when chosen, ink fill when focused.
 * Wrap a group's options in `Column(Modifier.selectableGroup())`, see
 * [PanelOptions].
 */
@Composable
fun PanelOption(
    text: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    FocusSurface(
        onClick = onClick,
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = IrisSize.option)
            .semantics {
                role = Role.RadioButton
                this.selected = selected
            },
        shape = IrisShape.card,
        colors = FocusColors.Quiet.copy(content = if (selected) IrisColor.accent else IrisColor.ink),
        contentAlignment = Alignment.CenterStart,
    ) { focused ->
        Row(
            Modifier.padding(horizontal = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(9.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RadioDot(selected = selected, color = if (focused) IrisColor.ground else if (selected) IrisColor.accent else IrisColor.inkMuted)
            Text(text, style = IrisType.control, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}

/**
 * A radio group of [PanelOption]s. The chosen option (the first when none is) carries
 * [selectedFocus]: a panel opens on what is chosen. [focusOnOpen] focuses it at once (a panel
 * with one group); with several groups, pass the group's [selectedFocus] and focus it yourself.
 */
@Composable
fun <T> PanelOptions(
    options: List<T>,
    selected: T?,
    onSelect: (T) -> Unit,
    label: (T) -> String,
    modifier: Modifier = Modifier,
    focusOnOpen: Boolean = false,
    selectedFocus: FocusRequester? = null,
) {
    val own = remember { FocusRequester() }
    val focus = selectedFocus ?: own
    val target = selected?.takeIf { it in options } ?: options.firstOrNull()
    if (focusOnOpen) LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    Column(modifier.selectableGroup(), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
        options.forEach { option ->
            PanelOption(
                label(option),
                selected = option == selected,
                onClick = { onSelect(option) },
                modifier = if (option == target) Modifier.focusRequester(focus) else Modifier,
            )
        }
    }
}

/**
 * Long text in a [SidePanel] (release notes, an NFO), one block per paragraph. Each block takes
 * focus, its ring around it, so the D-pad scrolls the text a paragraph at a time; the first is
 * focused when the panel opens.
 */
@Composable
fun PanelParagraphs(blocks: List<AnnotatedString>, style: TextStyle) {
    val first = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { first.requestFocus() } }
    blocks.forEachIndexed { i, block ->
        var focused by remember { mutableStateOf(false) }
        Text(
            block,
            style = style,
            color = IrisColor.ink,
            modifier = Modifier
                .fillMaxWidth()
                .padding(IrisFocus.ringWidth + IrisFocus.ringOffset)
                .then(if (i == 0) Modifier.focusRequester(first) else Modifier)
                .onFocusChanged { focused = it.isFocused }
                .focusRing(focused, IrisShape.key)
                .focusable()
                .padding(horizontal = 10.dp, vertical = IrisSpace.s1),
        )
        Spacer(Modifier.height(IrisSpace.s1))
    }
}

@Composable
private fun RadioDot(selected: Boolean, color: Color) {
    Box(
        Modifier
            .size(13.dp)
            .drawBehind {
                val stroke = 1.5.dp.toPx()
                drawCircle(color, radius = size.minDimension / 2 - stroke / 2, style = Stroke(stroke))
                if (selected) drawCircle(color, radius = size.minDimension * 0.25f)
            },
    )
}

/** The dimmed backdrop behind a centered dialog. */
fun Modifier.dialogScrim(): Modifier = background(IrisColor.overlay)

/** A centered dialog card on the [dialogScrim]: raised fill, a line, the panel radius. */
fun Modifier.dialogCard(): Modifier = background(IrisColor.groundRaised, IrisShape.panel)
    .border(1.dp, IrisColor.line, IrisShape.panel)
