package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.RadioButtonUnchecked
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Icon
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

enum class StepState { Done, Current, Pending }

/**
 * One step of getting ready. [detail] sits at the row's end ("38 peers",
 * "64 % · 8.2 MB/s"); [progress] (0..1) adds a meter under the label of the
 * current step: say the same value in [detail].
 */
@Immutable
data class Step(
    val label: String,
    val state: StepState,
    val detail: String? = null,
    val progress: Float? = null,
)

/**
 * The getting-ready checklist (TVPlayerStarting): done steps ticked, the
 * current one in the accent with a spinner, pending ones muted with an
 * empty ring. Announced politely as it changes.
 */
@Composable
fun StepList(steps: List<Step>, modifier: Modifier = Modifier) {
    Column(modifier.semantics { liveRegion = LiveRegionMode.Polite }) {
        steps.forEach { step -> StepRow(step) }
    }
}

@Composable
private fun StepRow(step: Step) {
    val color = when (step.state) {
        StepState.Done -> IrisColor.ink
        StepState.Current -> IrisColor.accent
        StepState.Pending -> IrisColor.inkMuted
    }
    Row(
        Modifier
            .heightIn(min = IrisSize.controlLarge)
            .bottomHairline()
            .semantics(mergeDescendants = true) {
                stateDescription = when (step.state) {
                    StepState.Done -> "Done"
                    StepState.Current -> "In progress"
                    StepState.Pending -> "Not started"
                }
            },
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(IrisSize.stepIcon), contentAlignment = Alignment.Center) {
            when (step.state) {
                StepState.Done -> Icon(Icons.Rounded.CheckCircle, null, tint = IrisColor.accent, modifier = Modifier.size(16.dp))
                StepState.Current -> Spinner(Modifier.size(14.dp), color = IrisColor.accent)
                StepState.Pending -> Icon(Icons.Rounded.RadioButtonUnchecked, null, tint = IrisColor.inkMuted, modifier = Modifier.size(16.dp))
            }
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Text(step.label, style = IrisType.body, color = color, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (step.progress != null && step.state == StepState.Current) {
                Meter(step.progress, Modifier.widthIn(max = 280.dp))
            }
        }
        if (step.detail != null) {
            Text(
                step.detail,
                style = IrisType.meta,
                color = if (step.state == StepState.Current) IrisColor.accent else IrisColor.inkMuted,
                maxLines = 1,
            )
        }
    }
}
