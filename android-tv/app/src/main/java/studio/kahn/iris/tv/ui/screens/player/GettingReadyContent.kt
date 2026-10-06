package studio.kahn.iris.tv.ui.screens.player

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Search
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Artwork
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.StepList
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** Everything the getting-ready screen draws (TVPlayerStarting). */
@Immutable
data class GettingReadyUi(
    val title: String,
    /** "S2:E1 · Hello, Ms. Cobel · English audio, French subtitles". */
    val subtitle: String?,
    val posterUrl: String?,
    val readiness: Readiness,
    /** The engine no longer has the release (404): offer to grab it again. */
    val gone: Boolean = false,
    val regrabFailed: Boolean = false,
    /** "regrab" or "replace" while that action runs. */
    val busy: String? = null,
    /** "The rest of the season keeps downloading in the background." */
    val note: String? = null,
)

/**
 * The stage before the picture: the poster, the title, each step in words
 * with how far the current one is (StepList), then Cancel and Pick another
 * release. A problem replaces the steps with what went wrong and what can be
 * done. Focus starts on Cancel (or on the problem's first action).
 */
@Composable
fun GettingReadyContent(
    ui: GettingReadyUi,
    onCancel: () -> Unit,
    onPickAnother: (deadSwarm: Boolean) -> Unit,
    onRetry: () -> Unit,
    onRegrab: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val layout = IrisLayout.current
    val first = remember { FocusRequester() }
    val problem = ui.readiness.problem
    // The buttons change with the problem and with a failed regrab: the focus follows.
    LaunchedEffect(ui.gone, problem?.title, ui.regrabFailed) { runCatching { first.requestFocus() } }
    Box(
        modifier
            .fillMaxSize()
            .background(IrisColor.stage)
            .padding(layout.safePadding),
        contentAlignment = Alignment.Center,
    ) {
        Row(
            Modifier.widthIn(max = 700.dp).fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s10),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Artwork(
                title = ui.title,
                imageUrl = ui.posterUrl,
                width = minOf(140.dp, layout.height * 0.42f),
                titleStyle = IrisType.group,
            )
            Column(
                Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    // Room for the focus ring inside the scroll's clip.
                    .padding(start = IrisSpace.s2, top = IrisSpace.s2, bottom = IrisSpace.s2),
                verticalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
                    Text("Getting ready", style = IrisType.meta, color = IrisColor.stageMuted)
                    Text(
                        ui.title,
                        style = IrisType.headline,
                        color = IrisColor.stageInk,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.semantics { heading() },
                    )
                    if (ui.subtitle != null) {
                        Text(ui.subtitle, style = IrisType.metaLarge, color = IrisColor.stageMuted, maxLines = 2)
                    }
                }
                when {
                    ui.gone -> GoneBlock(ui, first, onRegrab, onPickAnother, onCancel)
                    problem != null -> ProblemBlock(problem, ui.busy, first, onRetry, onPickAnother, onCancel)
                    else -> {
                        StepList(ui.readiness.steps)
                        Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
                            ActionButton("Cancel", onCancel, style = ActionStyle.Secondary, modifier = Modifier.focusRequester(first))
                            ActionButton(
                                "Pick another release",
                                { onPickAnother(false) },
                                style = ActionStyle.Secondary,
                                busy = ui.busy == WatchViewModel.BUSY_REPLACE,
                                busyText = "Opening search…",
                            )
                        }
                        if (ui.note != null) Text(ui.note, style = IrisType.meta, color = IrisColor.stageMuted)
                    }
                }
            }
        }
    }
}

@Composable
private fun ProblemBlock(
    problem: ReadyProblem,
    busy: String?,
    first: FocusRequester,
    onRetry: () -> Unit,
    onPickAnother: (Boolean) -> Unit,
    onCancel: () -> Unit,
) {
    Column(
        Modifier.semantics { liveRegion = LiveRegionMode.Assertive },
        verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
    ) {
        StatusLine(problem.title, tone = StatusTone.Down, style = IrisType.bodyStrong)
        Text(problem.detail, style = IrisType.body, color = IrisColor.stageMuted)
    }
    Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
        if (problem.deadSwarm) {
            ActionButton(
                "Remove it and pick another release",
                { onPickAnother(true) },
                icon = Icons.Rounded.Search,
                busy = busy == WatchViewModel.BUSY_REPLACE,
                busyText = "Removing…",
                modifier = Modifier.focusRequester(first),
            )
        } else {
            ActionButton("Try again", onRetry, icon = Icons.Rounded.Refresh, modifier = Modifier.focusRequester(first))
            ActionButton("Pick another release", { onPickAnother(false) }, style = ActionStyle.Secondary)
        }
        ActionButton("Cancel", onCancel, style = ActionStyle.Secondary)
    }
}

@Composable
private fun GoneBlock(
    ui: GettingReadyUi,
    first: FocusRequester,
    onRegrab: () -> Unit,
    onPickAnother: (Boolean) -> Unit,
    onCancel: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
        StatusLine("This file is no longer on disk", tone = StatusTone.Warn, style = IrisType.bodyStrong)
        Text(
            if (ui.regrabFailed) {
                "Iris could not grab this release again by itself. Pick another release instead."
            } else {
                "It was probably reclaimed to free up space. Grab it again and playback picks up where it left off."
            },
            style = IrisType.body,
            color = IrisColor.stageMuted,
        )
    }
    Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
        if (!ui.regrabFailed) {
            ActionButton(
                "Grab it again",
                onRegrab,
                icon = Icons.Rounded.Download,
                busy = ui.busy == WatchViewModel.BUSY_REGRAB,
                busyText = "Grabbing…",
                modifier = Modifier.focusRequester(first),
            )
            ActionButton("Pick another release", { onPickAnother(false) }, style = ActionStyle.Secondary)
        } else {
            ActionButton("Pick another release", { onPickAnother(false) }, modifier = Modifier.focusRequester(first))
        }
        ActionButton("Cancel", onCancel, style = ActionStyle.Secondary)
    }
}
