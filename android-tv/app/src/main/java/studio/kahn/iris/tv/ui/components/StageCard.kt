package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * The one card over the stage when playback stopped (a film, a channel): the [title] as a
 * down state, the [message], then "Try again" when [onRetry] can help and the way out
 * ([backLabel]). The first button takes the focus when the card shows.
 */
@Composable
fun StageErrorCard(
    title: String,
    message: String,
    onRetry: (() -> Unit)?,
    backLabel: String,
    onBack: () -> Unit,
    focus: FocusRequester = remember { FocusRequester() },
) {
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    Box(Modifier.fillMaxSize().background(IrisColor.overlay), contentAlignment = Alignment.Center) {
        Column(
            Modifier
                .padding(IrisSpace.s8)
                .widthIn(max = IrisSize.dialog)
                .background(IrisColor.stageScrim, IrisShape.panel)
                .padding(IrisSpace.s8),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s4),
        ) {
            StatusLine(title, tone = StatusTone.Down, style = IrisType.bodyStrong)
            Text(message, style = IrisType.body, color = IrisColor.stageMuted)
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
                if (onRetry != null) {
                    ActionButton("Try again", onRetry, icon = Icons.Rounded.Refresh, modifier = Modifier.focusRequester(focus))
                    ActionButton(backLabel, onBack, style = ActionStyle.Secondary)
                } else {
                    ActionButton(backLabel, onBack, modifier = Modifier.focusRequester(focus))
                }
            }
        }
    }
}
