package studio.kahn.iris.tv.ui.screens.player

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.FormatListBulleted
import androidx.compose.material.icons.rounded.ClosedCaption
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.IconAction
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.KeyHints
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** The top bar's words (TVPlayer): the title, the episode line, the facts. */
@Immutable
data class PlayerTitle(
    val title: String,
    val episode: String?,
    val facts: String,
)

/** Where the scrub bar stands. [previewMs]: a seek being chosen (held ←/→, a drag), drawn instead of the position. */
@Immutable
data class ScrubPosition(
    val positionMs: Long,
    val bufferedMs: Long,
    val durationMs: Long,
    val previewMs: Long? = null,
) {
    val shownMs: Long get() = previewMs ?: positionMs
}

/** The buttons row. Null labels hide their button. */
@Immutable
data class PlayerButtons(
    val playing: Boolean,
    val showTracks: Boolean,
    /** "Episodes" or "Other files" when there are several. */
    val sideLabel: String?,
    /** "Next: Trojan's Horse" past 95 % when it is on disk. */
    val nextLabel: String?,
    /** "English audio · English subtitles (SDH)", or a notice. */
    val trailing: String,
)

/** Focus handles of the buttons row, owned by the screen (it moves focus in on ↓, onto Next at the end). */
class PlayerButtonFocus {
    val play = FocusRequester()
    val next = FocusRequester()
}

/** The top bar: title, episode code and name, the facts and the clock on the right. */
@Composable
fun PlayerTopBar(info: PlayerTitle, clock: String?, modifier: Modifier = Modifier) {
    val layout = IrisLayout.current
    Row(
        modifier
            .fillMaxWidth()
            .background(IrisColor.stageScrim)
            .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, top = layout.safeVertical, bottom = 14.dp),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5)) {
            Text(
                info.title,
                style = IrisType.stageTitle,
                color = IrisColor.stageInk,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.alignByBaseline().weight(1f, fill = false),
            )
            if (info.episode != null) {
                Text(
                    info.episode,
                    style = IrisType.metaLarge,
                    color = IrisColor.stageMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.alignByBaseline(),
                )
            }
        }
        val facts = listOfNotNull(info.facts.ifBlank { null }, clock).joinToString(" · ")
        if (facts.isNotEmpty()) {
            Text(facts, style = IrisType.meta, color = IrisColor.stageMuted, maxLines = 1)
        }
    }
}

/**
 * The seek track (TVPlayer): elapsed time, the bar (buffered part, played
 * part, the thumb), the time left. Not focusable: ←/→ seek whenever the
 * buttons don't hold the focus. On a phone, a tap or a drag picks a point
 * ([onPreview] while dragging, [onSeek] on release).
 */
@Composable
fun ScrubBar(
    position: ScrubPosition,
    modifier: Modifier = Modifier,
    onPreview: ((Long?) -> Unit)? = null,
    onSeek: ((Long) -> Unit)? = null,
) {
    val duration = position.durationMs.coerceAtLeast(0)
    val shown = position.shownMs.coerceIn(0, if (duration > 0) duration else Long.MAX_VALUE)
    Row(
        modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            clockText(shown),
            style = IrisType.figure,
            color = IrisColor.stageInk,
            modifier = Modifier.widthIn(min = 45.dp),
            maxLines = 1,
        )
        val played = if (duration > 0) shown.toFloat() / duration else 0f
        val buffered = if (duration > 0) position.bufferedMs.toFloat() / duration else 0f
        val currentDuration by rememberUpdatedState(duration)
        val preview by rememberUpdatedState(onPreview)
        val seek by rememberUpdatedState(onSeek)
        var dragFraction by remember { mutableStateOf<Float?>(null) }
        Canvas(
            Modifier
                .weight(1f)
                .height(20.dp)
                .semantics {
                    progressBarRangeInfo = ProgressBarRangeInfo(played.coerceIn(0f, 1f), 0f..1f)
                    stateDescription = "${clockText(shown)} of ${clockText(duration)}"
                }
                .pointerInput(Unit) {
                    detectTapGestures { offset ->
                        val d = currentDuration
                        if (d > 0) seek?.invoke((offset.x / size.width).coerceIn(0f, 1f).times(d).toLong())
                    }
                }
                .pointerInput(Unit) {
                    detectHorizontalDragGestures(
                        onDragStart = { offset -> dragFraction = (offset.x / size.width).coerceIn(0f, 1f) },
                        onDragEnd = {
                            val f = dragFraction
                            dragFraction = null
                            val d = currentDuration
                            preview?.invoke(null)
                            if (f != null && d > 0) seek?.invoke((f * d).toLong())
                        },
                        onDragCancel = {
                            dragFraction = null
                            preview?.invoke(null)
                        },
                    ) { change, _ ->
                        change.consume()
                        val f = (change.position.x / size.width).coerceIn(0f, 1f)
                        dragFraction = f
                        val d = currentDuration
                        if (d > 0) preview?.invoke((f * d).toLong())
                    }
                },
        ) {
            val trackHeight = 5.dp.toPx()
            val y = (size.height - trackHeight) / 2f
            val radius = CornerRadius(trackHeight / 2f)
            drawRoundRect(IrisColor.stageLine, Offset(0f, y), Size(size.width, trackHeight), radius)
            drawRoundRect(IrisColor.stageFill, Offset(0f, y), Size(size.width * buffered.coerceIn(0f, 1f), trackHeight), radius)
            val x = size.width * played.coerceIn(0f, 1f)
            drawRoundRect(IrisColor.accent, Offset(0f, y), Size(x, trackHeight), radius)
            val thumb = 9.dp.toPx()
            drawCircle(IrisColor.accent, radius = thumb, center = Offset(x, size.height / 2f))
            drawCircle(IrisColor.stageInk, radius = thumb - 2.dp.toPx(), center = Offset(x, size.height / 2f))
        }
        Text(
            if (duration > 0) remainingText(shown, duration) else "",
            style = IrisType.figure,
            color = IrisColor.stageMuted,
            textAlign = TextAlign.End,
            modifier = Modifier.widthIn(min = 55.dp),
            maxLines = 1,
        )
    }
}

/**
 * The bottom bar (TVPlayer): the scrub bar, the buttons (play/pause, Audio
 * and subtitles, Episodes, Next) with the tracks summary at the end, and the
 * remote's keys. ↑ from the buttons gives ←/→ back to seeking ([onLeaveButtons]).
 */
@Composable
fun PlayerBottomBar(
    scrub: ScrubPosition,
    buttons: PlayerButtons,
    focus: PlayerButtonFocus,
    onPlayPause: () -> Unit,
    onTracks: () -> Unit,
    onSide: () -> Unit,
    onNext: () -> Unit,
    onLeaveButtons: () -> Unit,
    modifier: Modifier = Modifier,
    onPreview: ((Long?) -> Unit)? = null,
    onSeek: ((Long) -> Unit)? = null,
) {
    val layout = IrisLayout.current
    Column(
        modifier
            .fillMaxWidth()
            .background(IrisColor.stageScrim)
            .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, top = 16.dp, bottom = 23.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        ScrubBar(scrub, onPreview = onPreview, onSeek = onSeek)
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Row(
                Modifier
                    .focusRestorer(focus.play)
                    .focusGroup()
                    .onPreviewKeyEvent { event ->
                        if (event.key == Key.DirectionUp && event.type == KeyEventType.KeyDown) {
                            onLeaveButtons()
                            true
                        } else {
                            false
                        }
                    },
                horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconAction(
                    icon = if (buttons.playing) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                    contentDescription = if (buttons.playing) "Pause" else "Play",
                    onClick = onPlayPause,
                    size = IrisSize.controlLarge,
                    modifier = Modifier.focusRequester(focus.play),
                )
                if (buttons.showTracks) {
                    ActionButton(
                        "Audio and subtitles",
                        onTracks,
                        style = ActionStyle.Quiet,
                        size = ActionSize.Large,
                        icon = Icons.Rounded.ClosedCaption,
                    )
                }
                if (buttons.sideLabel != null) {
                    ActionButton(
                        buttons.sideLabel,
                        onSide,
                        style = ActionStyle.Quiet,
                        size = ActionSize.Large,
                        icon = Icons.AutoMirrored.Rounded.FormatListBulleted,
                    )
                }
                if (buttons.nextLabel != null) {
                    ActionButton(
                        buttons.nextLabel,
                        onNext,
                        style = ActionStyle.Quiet,
                        size = ActionSize.Large,
                        icon = Icons.Rounded.SkipNext,
                        modifier = Modifier.focusRequester(focus.next).widthIn(max = 260.dp),
                    )
                }
            }
            Box(Modifier.weight(1f), contentAlignment = Alignment.CenterEnd) {
                Text(
                    buttons.trailing,
                    style = IrisType.meta,
                    color = IrisColor.stageMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    textAlign = TextAlign.End,
                )
            }
        }
        KeyHints(
            listOf(
                KeyHint(Keys.LEFT_RIGHT, "Back or forward 10 s, hold to go faster"),
                KeyHint(Keys.DOWN, "To these buttons"),
                KeyHint(Keys.BACK, "Hide, then leave"),
            ),
            onStage = true,
        )
    }
}

/** The whole chrome over the picture: the top bar and the bottom bar. */
@Composable
fun PlayerControls(
    title: PlayerTitle,
    clock: String?,
    scrub: ScrubPosition,
    buttons: PlayerButtons,
    focus: PlayerButtonFocus,
    onPlayPause: () -> Unit,
    onTracks: () -> Unit,
    onSide: () -> Unit,
    onNext: () -> Unit,
    onLeaveButtons: () -> Unit,
    modifier: Modifier = Modifier,
    onPreview: ((Long?) -> Unit)? = null,
    onSeek: ((Long) -> Unit)? = null,
) {
    Box(modifier.fillMaxSize()) {
        PlayerTopBar(title, clock, Modifier.align(Alignment.TopStart))
        PlayerBottomBar(
            scrub = scrub,
            buttons = buttons,
            focus = focus,
            onPlayPause = onPlayPause,
            onTracks = onTracks,
            onSide = onSide,
            onNext = onNext,
            onLeaveButtons = onLeaveButtons,
            onPreview = onPreview,
            onSeek = onSeek,
            modifier = Modifier.align(Alignment.BottomStart),
        )
    }
}
