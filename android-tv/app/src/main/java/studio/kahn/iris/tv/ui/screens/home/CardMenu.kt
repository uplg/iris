package studio.kahn.iris.tv.ui.screens.home

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.dialogCard
import studio.kahn.iris.tv.ui.components.dialogScrim
import studio.kahn.iris.tv.ui.components.touchClick
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * The "Hold OK" menu of a card (web `CardMenu`): what one can do with it, as a column of
 * actions on the dialog scrim, the first focused. Opened by hold OK on the remote and by a
 * long press on a phone. [busy] = the action that travels: it stays in place saying so
 * (and every action ignores presses meanwhile); the caller closes the menu on the server's
 * answer. Back, a tap on the scrim or Cancel call [onDismiss].
 *
 * Candidate for `ui/components`: [studio.kahn.iris.tv.ui.components.ConfirmDialog] with
 * several actions.
 */
@Composable
fun CardMenu(
    eyebrow: String,
    title: String,
    actions: List<CardAction>,
    onAction: (CardAction) -> Unit,
    onDismiss: () -> Unit,
    busy: CardAction? = null,
) {
    BackHandler(onBack = onDismiss)
    val first = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { first.requestFocus() } }
    // Opened by a hold, with OK still down: its release must not press the first action.
    var openingPressReleased by remember { mutableStateOf(false) }
    Box(
        Modifier
            .fillMaxSize()
            .dialogScrim()
            .onPreviewKeyEvent { event ->
                if (event.key == Key.Back) {
                    if (event.type == KeyEventType.KeyUp) onDismiss()
                    return@onPreviewKeyEvent true
                }
                if (openingPressReleased) return@onPreviewKeyEvent false
                val select = event.key == Key.DirectionCenter || event.key == Key.Enter || event.key == Key.NumPadEnter
                if (select && event.type == KeyEventType.KeyUp) openingPressReleased = true
                select
            }
            .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClick = onDismiss),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            Modifier
                .widthIn(max = 420.dp)
                .width(IntrinsicSize.Max)
                .dialogCard()
                .touchClick {}
                .padding(IrisSpace.s8),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        ) {
            Eyebrow(eyebrow)
            Text(title, style = IrisType.group, color = IrisColor.ink, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Column(
                Modifier.padding(top = IrisSpace.s2),
                verticalArrangement = Arrangement.spacedBy(IrisSpace.s2),
            ) {
                actions.forEachIndexed { i, action ->
                    ActionButton(
                        action.label,
                        onClick = { if (busy == null) onAction(action) },
                        style = if (i == 0) ActionStyle.Primary else ActionStyle.Secondary,
                        busy = busy == action,
                        busyText = action.busyLabel,
                        modifier = Modifier
                            .fillMaxWidth()
                            .then(if (i == 0) Modifier.focusRequester(first) else Modifier),
                    )
                }
                ActionButton(
                    "Cancel",
                    onDismiss,
                    style = ActionStyle.Secondary,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}
