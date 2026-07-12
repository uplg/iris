package studio.kahn.iris.tv.ui.components

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing

 /**
  * Scrim + centered card confirmation (same shape as the Continue
  * Watching manage sheet). Back or the scrim cancels; focus lands on
  * the CONFIRM button.
  */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun ConfirmDialog(
    eyebrow: String,
    title: String,
    confirmLabel: String,
    onConfirm: () -> Unit,
    onCancel: () -> Unit,
    body: String? = null,
) {
    BackHandler(enabled = true, onBack = onCancel)
    val confirmFocus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { confirmFocus.requestFocus() } }
    // The dialog usually opens from a LONG-PRESS, with OK still held:
    // its release (and auto-repeat downs) would land on the confirm
    // button and click it instantly. Swallow every Select event until
    // the opening press fully releases.
    var openingPressReleased by remember { mutableStateOf(false) }
    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black.copy(alpha = 0.7f))
            .onPreviewKeyEvent { event ->
                // Back cancels, handled at the key level — the dispatcher route
                // proved unreliable with focus inside the dialog (first press
                // eaten). The BackHandler below stays as a fallback.
                if (event.key == Key.Back) {
                    if (event.type == KeyEventType.KeyUp) onCancel()
                    return@onPreviewKeyEvent true
                }
                if (openingPressReleased) return@onPreviewKeyEvent false
                val isSelect = event.key == Key.DirectionCenter ||
                    event.key == Key.Enter ||
                    event.key == Key.NumPadEnter
                if (isSelect) {
                    if (event.type == KeyEventType.KeyUp) openingPressReleased = true
                    true
                } else {
                    false
                }
            }
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClick = onCancel,
            ),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            Modifier
                .widthIn(max = 460.dp)
                .background(IrisColors.Elev2, RoundedCornerShape(Radius.lg))
                .padding(Spacing.xl),
            verticalArrangement = Arrangement.spacedBy(Spacing.md),
        ) {
            Eyebrow(eyebrow)
            Text(
                title,
                style = MaterialTheme.typography.titleMedium,
                color = IrisColors.Foreground,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            if (body != null) {
                Text(
                    body,
                    style = MaterialTheme.typography.bodySmall,
                    color = IrisColors.MutedForeground,
                )
            }
            // Intrinsic-width buttons side by side — a fillMaxWidth
            // button wears a card-wide focus ring, which reads as a
            // giant highlight instead of a button.
            Row(horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
                IrisButton(
                    confirmLabel,
                    onConfirm,
                    modifier = Modifier.focusRequester(confirmFocus),
                )
                IrisButton(
                    "Cancel",
                    onCancel,
                    variant = IrisButtonVariant.Ghost,
                )
            }
        }
    }
}
