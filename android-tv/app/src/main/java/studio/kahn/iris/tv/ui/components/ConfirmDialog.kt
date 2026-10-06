package studio.kahn.iris.tv.ui.components

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.text.style.TextOverflow
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * The one dialog shell: a centered card on the dialog scrim, the D-pad kept inside, Back and a
 * tap on the scrim calling [onCancel]; [eyebrow], [title] and an optional [body] head it.
 */
@Composable
fun DialogShell(
    eyebrow: String,
    title: String,
    onCancel: () -> Unit,
    body: String? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    BackHandler(enabled = true, onBack = onCancel)
    Box(
        Modifier
            .fillMaxSize()
            .dialogScrim()
            .dialogKeys(onCancel)
            .touchClick(onClick = onCancel),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            Modifier
                .widthIn(max = IrisSize.dialog)
                .dialogCard()
                // Swallow touch taps on the card body — without this they
                // bubble to the scrim's clickable and dismiss the dialog.
                .touchClick {}
                // The D-pad stays in the dialog: nothing behind the scrim takes the focus.
                .focusProperties { onExit = { cancelFocusChange() } }
                .focusGroup()
                .padding(IrisSpace.s8),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s5),
        ) {
            Eyebrow(eyebrow)
            Text(title, style = IrisType.group, color = IrisColor.ink, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (body != null) Text(body, style = IrisType.meta, color = IrisColor.inkMuted)
            content()
        }
    }
}

/** [confirmLabel] (busy while [busy]) and Cancel, side by side. */
@Composable
fun DialogButtons(
    confirmLabel: String,
    onConfirm: () -> Unit,
    onCancel: () -> Unit,
    confirmModifier: Modifier = Modifier,
    busy: Boolean = false,
    busyText: String? = null,
) {
    // Intrinsic-width buttons side by side — a fillMaxWidth
    // button wears a card-wide focus ring, which reads as a
    // giant highlight instead of a button.
    Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
        ActionButton(confirmLabel, onConfirm, busy = busy, busyText = busyText, modifier = confirmModifier)
        ActionButton("Cancel", onCancel, style = ActionStyle.Secondary)
    }
}

/**
 * A centered confirmation ([DialogShell]): the confirm action (focused on open) and Cancel.
 * Back, a tap on the scrim or Cancel call [onCancel]. Safe to open from a hold-OK: the
 * still-held OK cannot confirm it.
 */
@Composable
fun ConfirmDialog(
    eyebrow: String,
    title: String,
    confirmLabel: String,
    onConfirm: () -> Unit,
    onCancel: () -> Unit,
    body: String? = null,
) {
    val confirmFocus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { confirmFocus.requestFocus() } }
    DialogShell(eyebrow, title, onCancel, body) {
        DialogButtons(confirmLabel, onConfirm, onCancel, Modifier.focusRequester(confirmFocus))
    }
}

/**
 * A centered dialog with fields ([content]: [TextInput]s) between its head and [DialogButtons].
 * [firstFocus] is focused on open: pass the first field's requester.
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
    LaunchedEffect(Unit) { runCatching { firstFocus.requestFocus() } }
    DialogShell(eyebrow, title, onCancel, body) {
        content()
        DialogButtons(confirmLabel, onConfirm, onCancel, busy = busy, busyText = busyText)
    }
}
