package studio.kahn.iris.tv.ui.screens.player

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Icon
import androidx.tv.material3.Text
import java.util.Locale
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.FocusColors
import studio.kahn.iris.tv.ui.components.FocusSurface
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.KeyHints
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.LanguageWords
import studio.kahn.iris.tv.ui.components.Meter
import studio.kahn.iris.tv.ui.components.PanelLabel
import studio.kahn.iris.tv.ui.components.PanelOption
import studio.kahn.iris.tv.ui.components.SidePanel
import studio.kahn.iris.tv.ui.components.Spinner
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** Said under the track options: where the choice is kept (the web's `keptForText`). */
fun keptForText(forSeries: Boolean): String =
    (if (forSeries) "Kept for the whole series." else "Kept as your default.") +
        " Subtitle size and colors follow the Android caption settings."

/**
 * Audio and subtitles (TVPlayerTracks): two radio groups in a side panel, the
 * playing tracks chosen, focus on the chosen subtitle (else the chosen
 * audio). Back closes it, the video keeps playing.
 */
@Composable
fun TracksPanel(
    menu: TrackMenu,
    forSeries: Boolean,
    onChoose: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val initial = remember { FocusRequester() }
    val focusId = menu.subtitles.firstOrNull { it.selected && it.id != SUBTITLES_OFF }?.id
        ?: menu.audio.firstOrNull { it.selected }?.id
        ?: menu.subtitles.firstOrNull()?.id
    LaunchedEffect(Unit) { runCatching { initial.requestFocus() } }
    Box(Modifier.fillMaxSize()) {
        PanelKeyHints(listOf(KeyHint(Keys.OK, "Choose"), KeyHint(Keys.BACK, "Close, the video keeps playing")))
        SidePanel(title = "Audio and subtitles", onDismiss = onDismiss, footer = keptForText(forSeries)) {
            if (menu.audio.isNotEmpty()) {
                PanelLabel("Audio")
                ChoiceGroup(menu.audio, focusId, initial, onChoose)
            }
            PanelLabel("Subtitles", Modifier.padding(top = IrisSpace.s3))
            ChoiceGroup(menu.subtitles, focusId, initial, onChoose)
        }
    }
}

@Composable
private fun ChoiceGroup(
    choices: List<TrackChoice>,
    focusId: String?,
    initial: FocusRequester,
    onChoose: (String) -> Unit,
) {
    Column(Modifier.selectableGroup(), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
        choices.forEach { c ->
            PanelOption(
                c.label,
                selected = c.selected,
                onClick = { onChoose(c.id) },
                modifier = if (c.id == focusId) Modifier.focusRequester(initial) else Modifier,
            )
        }
    }
}

/**
 * The season beside the player (or the torrent's other files): what is
 * watched, where one stopped, the one playing now, and the way to each
 * (play, or grab one only discovered). Focus starts on the playing row.
 */
@Composable
fun EpisodesPanel(
    title: String,
    rows: List<SideRow>,
    busyKey: String?,
    onPlay: (SideRow) -> Unit,
    onGrab: (SideRow) -> Unit,
    onDismiss: () -> Unit,
) {
    val initial = remember { FocusRequester() }
    val focusKey = rows.firstOrNull { it.active }?.key ?: rows.firstOrNull()?.key
    LaunchedEffect(Unit) { runCatching { initial.requestFocus() } }
    Box(Modifier.fillMaxSize()) {
        PanelKeyHints(listOf(KeyHint(Keys.OK, "Play"), KeyHint(Keys.BACK, "Close, the video keeps playing")))
        SidePanel(title = "$title (${rows.size})", onDismiss = onDismiss) {
            rows.forEach { row ->
                EpisodeRow(
                    row = row,
                    busy = busyKey == row.key,
                    onClick = {
                        when {
                            row.active -> onDismiss()
                            row.grab != null -> onGrab(row)
                            else -> onPlay(row)
                        }
                    },
                    modifier = if (row.key == focusKey) Modifier.focusRequester(initial) else Modifier,
                )
            }
        }
    }
}

@Composable
private fun EpisodeRow(row: SideRow, busy: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val facts = buildList {
        if (row.secondary.isNotEmpty()) add(if (row.mono) row.secondary else LanguageWords.of(row.secondary) ?: row.secondary)
        if (row.watched) add("Watched")
        if (row.started) add(String.format(Locale.ROOT, "%d %% watched", row.watchedPct?.toInt() ?: 0))
        if (row.active) add("Now playing")
        if (row.grab != null) add(if (busy) "Grabbing…" else "Not downloaded yet")
    }.joinToString(" · ")
    FocusSurface(
        onClick = { if (!busy) onClick() },
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = IrisSize.option)
            .semantics { if (row.active) stateDescription = "Now playing" },
        shape = IrisShape.card,
        colors = FocusColors.Quiet.copy(content = if (row.active) IrisColor.accent else IrisColor.ink),
        contentAlignment = Alignment.CenterStart,
    ) { focused ->
        Row(
            Modifier.padding(horizontal = 10.dp, vertical = 7.dp),
            horizontalArrangement = Arrangement.spacedBy(9.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(13.dp), contentAlignment = Alignment.Center) {
                when {
                    busy -> Spinner(Modifier.size(12.dp))
                    row.grab != null -> Icon(Icons.Rounded.Download, contentDescription = null, modifier = Modifier.size(13.dp))
                    row.active -> Icon(Icons.Rounded.PlayArrow, contentDescription = null, modifier = Modifier.size(13.dp))
                }
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(
                    row.primary,
                    style = if (row.mono) IrisType.mono else IrisType.control,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (facts.isNotEmpty()) {
                    Text(
                        facts,
                        style = IrisType.metaSmall,
                        color = if (focused) IrisColor.inkMutedOnInk else IrisColor.inkMuted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                if (row.started) {
                    Meter(((row.watchedPct ?: 0.0) / 100.0).toFloat(), height = 2.dp, track = IrisColor.line)
                }
            }
        }
    }
}

/** The remote's keys at the bottom left while a side panel is open (TVPlayerTracks). */
@Composable
private fun PanelKeyHints(hints: List<KeyHint>) {
    val layout = IrisLayout.current
    Box(Modifier.fillMaxSize().padding(start = layout.safeHorizontal, bottom = 20.dp), contentAlignment = Alignment.BottomStart) {
        KeyHints(hints, onStage = true, modifier = Modifier.fillMaxWidth(0.5f))
    }
}

/** Over the stage when playback stopped on an error: the reason and the ways out. */
@Composable
fun PlayerErrorNotice(
    message: String,
    onRetry: () -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val retry = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { retry.requestFocus() } }
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(
            Modifier
                .padding(IrisSpace.s8)
                .widthIn(max = 460.dp)
                .background(IrisColor.stageScrim, IrisShape.panel)
                .padding(IrisSpace.s8),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s4),
        ) {
            StatusLine("The player stopped", tone = StatusTone.Down, style = IrisType.bodyStrong)
            Text(message, style = IrisType.body, color = IrisColor.stageInk)
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
                ActionButton("Try again", onRetry, modifier = Modifier.focusRequester(retry))
                ActionButton("Back", onBack, style = ActionStyle.Secondary)
            }
        }
    }
}
