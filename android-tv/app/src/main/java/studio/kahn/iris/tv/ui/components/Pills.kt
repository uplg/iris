package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Icon
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * A filter or view pill (TVLibrary, TVSearchGrid). [selected]: accent frame,
 * accent wash, accent words AND a tick, so the state never rests on color
 * alone. Alone it is a toggle (announced as a checkbox); inside [PillChoice]
 * it is one radio of a group.
 */
@Composable
fun Pill(
    text: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    role: Role = Role.Checkbox,
) {
    FocusSurface(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier
            .heightIn(min = IrisSize.pill)
            .semantics {
                this.role = role
                if (role == Role.RadioButton) {
                    this.selected = selected
                } else {
                    toggleableState = ToggleableState(selected)
                }
            },
        shape = IrisShape.pill,
        colors = if (selected) {
            FocusColors.Outlined.copy(
                container = IrisColor.accentWash,
                content = IrisColor.accent,
                border = IrisColor.accent,
            )
        } else {
            FocusColors.Outlined.copy(content = IrisColor.inkMuted)
        },
    ) {
        Row(
            Modifier.padding(start = if (selected) 8.dp else 11.dp, end = 11.dp),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s1),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (selected) Icon(Icons.Rounded.Check, contentDescription = null, modifier = Modifier.size(10.dp))
            Text(text, style = IrisType.controlSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

/**
 * One choice among a few (view: Titles / Grid / List; a season; an audio
 * language). A radio group: the chosen [Pill] is filled and ticked.
 * Coming back into the group with the D-pad lands on the pill focused last.
 */
@Composable
fun <T> PillChoice(
    options: List<T>,
    selected: T,
    onSelect: (T) -> Unit,
    label: (T) -> String,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier
            .selectableGroup()
            .focusRestorer()
            .focusGroup(),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        options.forEach { option ->
            Pill(
                text = label(option),
                selected = option == selected,
                onClick = { onSelect(option) },
                role = Role.RadioButton,
            )
        }
    }
}

/** A chip's state. Its words carry the meaning; the tone only supports them. */
enum class ChipTone(val container: Color, val content: Color) {
    Neutral(IrisColor.groundRaised, IrisColor.inkMuted),
    Ok(IrisColor.accentWash, IrisColor.accent),
    Warn(IrisColor.downWash, IrisColor.warn),
    Down(IrisColor.downWash, IrisColor.down),
}

/** 40 px (release facts) or 36 px (inside a list row). */
enum class ChipSize { Regular, Small }

/** A non-interactive fact: "Freeleech", "V3X", "English audio, French subtitles". */
@Composable
fun Chip(
    text: String,
    modifier: Modifier = Modifier,
    tone: ChipTone = ChipTone.Neutral,
    size: ChipSize = ChipSize.Regular,
) {
    val small = size == ChipSize.Small
    Box(
        modifier
            .heightIn(min = if (small) IrisSize.chipSmall else IrisSize.chip)
            .background(tone.container, IrisShape.pill)
            .padding(horizontal = if (small) 7.dp else 8.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text,
            style = if (small) IrisType.metaSmall else IrisType.chip,
            color = tone.content,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** A release's language tag (`french`, `english`, `multi`…) said in words. */
object LanguageWords {
    fun of(language: String?): String? = when (language?.lowercase()) {
        null, "", "unknown" -> null
        "french" -> "French"
        "english" -> "English"
        "multi" -> "Multi-language"
        else -> language.uppercase()
    }
}

/** [Chip] for a release language; nothing when the tag is unknown. */
@Composable
fun LanguageChip(language: String?, modifier: Modifier = Modifier, size: ChipSize = ChipSize.Small) {
    val words = LanguageWords.of(language) ?: return
    Chip(words, modifier, size = size)
}
