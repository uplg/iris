package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** What an action ended with, in words and a tone, said once on the screen it happened on. */
@Immutable
data class Notice(val text: String, val tone: StatusTone = StatusTone.Muted) {
    /** A done action ([StatusTone.Ok]) or a failed one ([StatusTone.Down]). */
    constructor(text: String, failed: Boolean) : this(text, if (failed) StatusTone.Down else StatusTone.Ok)

    val failed: Boolean get() = tone == StatusTone.Down
}

/**
 * The [notice] of the last action, nothing without one. Alone it is a [StatusLine] announced
 * politely. With [onClose] it is a framed banner that must be read: the notice, its [detail]
 * below, and a Close that takes the focus ([takeFocus]), announced at once. A banner over a
 * scrolled list leaves the focus where it is (the list would jump to its top).
 */
@Composable
fun NoticeLine(
    notice: Notice?,
    modifier: Modifier = Modifier,
    detail: String? = null,
    onClose: (() -> Unit)? = null,
    takeFocus: Boolean = true,
) {
    if (notice == null) return
    if (onClose == null) {
        StatusLine(notice.text, modifier.semantics { liveRegion = LiveRegionMode.Polite }, tone = notice.tone)
        return
    }
    val close = remember { FocusRequester() }
    LaunchedEffect(notice, detail) { if (takeFocus) runCatching { close.requestFocus() } }
    FramedBlock(
        modifier
            .fillMaxWidth()
            .semantics { liveRegion = LiveRegionMode.Assertive },
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
                StatusLine(notice.text, tone = notice.tone, style = IrisType.bodyStrong)
                if (detail != null) {
                    Text(detail, style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 3, overflow = TextOverflow.Ellipsis)
                }
            }
            ActionButton("Close", onClose, style = ActionStyle.Secondary, size = ActionSize.Small, modifier = Modifier.focusRequester(close))
        }
    }
}
