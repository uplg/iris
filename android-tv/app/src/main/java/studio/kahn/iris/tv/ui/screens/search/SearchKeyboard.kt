package studio.kahn.iris.tv.ui.screens.search

import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.components.FocusColors
import studio.kahn.iris.tv.ui.components.FocusSurface
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisType

/** One key of [SearchKeyboard]: [span] columns of the 7. */
private data class KeySpec(val label: String, val span: Int = 1, val spoken: String = label, val action: KeyAction)

private sealed interface KeyAction {
    data class Type(val text: String) : KeyAction
    data object Space : KeyAction
    data object Delete : KeyAction
    data object Clear : KeyAction
    data object Toggle : KeyAction
}

private fun letters(row: String) = row.map { KeySpec(it.toString(), action = KeyAction.Type(it.toString())) }

private val LETTERS = listOf(
    letters("abcdefg"),
    letters("hijklmn"),
    letters("opqrstu"),
    letters("vwxyz") + KeySpec("123", span = 2, spoken = "Numbers and signs", action = KeyAction.Toggle),
)

private val SIGNS = listOf(
    letters("1234567"),
    letters("890-'.:"),
    letters("&!?,()+"),
    letters("éèàçô") + KeySpec("abc", span = 2, spoken = "Letters", action = KeyAction.Toggle),
)

private val LAST_ROW = listOf(
    KeySpec("Space", span = 3, action = KeyAction.Space),
    KeySpec("Delete", span = 2, spoken = "Delete the last character", action = KeyAction.Delete),
    KeySpec("Clear", span = 2, spoken = "Clear the field", action = KeyAction.Clear),
)

/** The keyboard's width: 7 keys and their 6 gaps. */
val KEYBOARD_WIDTH: Dp = IrisSize.control * 7 + KEY_GAP * 6

private val KEY_GAP get() = 4.dp

/**
 * The on-screen keyboard of the search boards (TVSearchStart): seven keys a
 * row, letters or numbers and signs, then Space, Delete and Clear. It types
 * into the field with the D-pad, so nobody has to bring up the system
 * keyboard on a TV; the field itself still opens the system one (and its
 * voice key) on OK. Coming back to it lands on the key used last.
 */
@Composable
fun SearchKeyboard(
    onType: (String) -> Unit,
    onDelete: () -> Unit,
    onClear: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var signs by rememberSaveable { mutableStateOf(false) }
    Column(
        modifier
            .width(KEYBOARD_WIDTH)
            .focusRestorer()
            .focusGroup(),
        verticalArrangement = Arrangement.spacedBy(KEY_GAP),
    ) {
        ((if (signs) SIGNS else LETTERS) + listOf(LAST_ROW)).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(KEY_GAP)) {
                row.forEach { key ->
                    Key(key) {
                        when (val action = key.action) {
                            is KeyAction.Type -> onType(action.text)
                            KeyAction.Space -> onType(" ")
                            KeyAction.Delete -> onDelete()
                            KeyAction.Clear -> onClear()
                            KeyAction.Toggle -> signs = !signs
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Key(key: KeySpec, onClick: () -> Unit) {
    val width = IrisSize.control * key.span + KEY_GAP * (key.span - 1)
    FocusSurface(
        onClick = onClick,
        modifier = Modifier
            .width(width)
            .height(IrisSize.control)
            .semantics { contentDescription = key.spoken },
        shape = IrisShape.control,
        colors = FocusColors.Outlined.copy(container = IrisColor.surface),
    ) {
        Box {
            Text(
                key.label,
                style = if (key.span > 1) IrisType.controlSmall else IrisType.action.copy(fontSize = 13.sp),
                maxLines = 1,
            )
        }
    }
}
