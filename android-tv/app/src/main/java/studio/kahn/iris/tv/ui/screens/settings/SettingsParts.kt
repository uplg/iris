package studio.kahn.iris.tv.ui.screens.settings

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.focusable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Icon
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.FocusColors
import studio.kahn.iris.tv.ui.components.FocusSurface
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.dialogCard
import studio.kahn.iris.tv.ui.components.dialogScrim
import studio.kahn.iris.tv.ui.components.focusRing
import studio.kahn.iris.tv.ui.components.touchClick
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * One entry of the Settings rail: left-aligned words, the accent wash when
 * it is the section shown (announced as selected), ink when focused.
 */
@Composable
fun RailItem(
    text: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    FocusSurface(
        onClick = onClick,
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = IrisSize.controlSmall)
            .semantics {
                role = Role.Tab
                this.selected = selected
            },
        shape = IrisShape.pill,
        colors = FocusColors.Quiet.copy(
            container = if (selected) IrisColor.accentWash else Color.Transparent,
            content = if (selected) IrisColor.accent else IrisColor.inkMuted,
        ),
        contentAlignment = Alignment.CenterStart,
    ) {
        Text(
            text,
            style = IrisType.control,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(horizontal = 12.dp),
        )
    }
}

/** A section's heading and what it is for, then its [content]. */
@Composable
fun SettingsGroup(
    title: String,
    hint: String?,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(IrisSpace.s5)) {
        Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
            Text(title, style = IrisType.panel, color = IrisColor.ink, modifier = Modifier.semantics { heading() })
            if (hint != null) Text(hint, style = IrisType.meta, color = IrisColor.inkMuted)
        }
        content()
    }
}

/** A label above a group of choices ("Languages"), with its hint. */
@Composable
fun ChoiceLabel(text: String, hint: String?) {
    Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
        Text(text, style = IrisType.group, color = IrisColor.ink, modifier = Modifier.semantics { heading() })
        if (hint != null) Text(hint, style = IrisType.meta, color = IrisColor.inkMuted)
    }
}

/**
 * A setting with its current value, changed by pressing it ("Audio · French
 * · Change"): a framed row, raised and ringed when focused.
 */
@Composable
fun SettingRow(
    label: String,
    value: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    action: String = "Change",
    enabled: Boolean = true,
) {
    FocusSurface(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = IrisSize.option),
        shape = IrisShape.card,
        colors = FocusColors.Row,
        contentAlignment = Alignment.CenterStart,
    ) {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                label,
                style = IrisType.meta,
                color = IrisColor.inkMuted,
                modifier = Modifier.widthIn(min = IrisSize.factLabel, max = IrisSize.factLabel),
            )
            Text(value, style = IrisType.body, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            Text(action, style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 1)
            Icon(Icons.AutoMirrored.Rounded.KeyboardArrowRight, contentDescription = null, tint = IrisColor.inkMuted, modifier = Modifier.size(IrisSize.icon))
        }
    }
}

/**
 * A row that only reads (a passkey): it takes focus, so the D-pad can
 * scroll a long list, and says it by its ring; it does nothing on OK.
 */
@Composable
fun ReadRow(
    modifier: Modifier = Modifier,
    content: @Composable RowScope.() -> Unit,
) {
    val source = remember { MutableInteractionSource() }
    val focused by source.collectIsFocusedAsState()
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = IrisSize.option)
            .focusRing(focused, IrisShape.card)
            .background(if (focused) IrisColor.groundRaised else IrisColor.surface, IrisShape.card)
            .border(1.dp, IrisColor.line, IrisShape.card)
            .focusable(interactionSource = source)
            .padding(horizontal = 12.dp, vertical = IrisSpace.s3),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

/**
 * A centered dialog with fields ([content]: [studio.kahn.iris.tv.ui.components.TextInput]s),
 * then [confirmLabel] and Cancel. Back, a tap on the scrim or Cancel call
 * [onCancel]. The D-pad cannot leave it. [firstFocus] is focused on open:
 * pass the first field's requester.
 */
@Composable
fun FormDialog(
    eyebrow: String,
    title: String,
    confirmLabel: String,
    onConfirm: () -> Unit,
    onCancel: () -> Unit,
    firstFocus: FocusRequester,
    busy: Boolean = false,
    busyText: String? = null,
    body: String? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    BackHandler(onBack = onCancel)
    LaunchedEffect(Unit) { runCatching { firstFocus.requestFocus() } }
    Box(
        Modifier
            .fillMaxSize()
            .dialogScrim()
            .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClick = onCancel),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            Modifier
                .widthIn(max = 460.dp)
                .dialogCard()
                .touchClick {}
                .focusProperties { onExit = { cancelFocusChange() } }
                .focusGroup()
                .padding(IrisSpace.s8),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s5),
        ) {
            Eyebrow(eyebrow)
            Text(title, style = IrisType.group, color = IrisColor.ink)
            if (body != null) Text(body, style = IrisType.meta, color = IrisColor.inkMuted)
            content()
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
                ActionButton(confirmLabel, onConfirm, busy = busy, busyText = busyText)
                ActionButton("Cancel", onCancel, style = ActionStyle.Secondary)
            }
        }
    }
}

/** Focuses this element when [Modifier.focusRequester] would, used for a field. */
fun Modifier.requester(requester: FocusRequester?): Modifier =
    if (requester != null) focusRequester(requester) else this
