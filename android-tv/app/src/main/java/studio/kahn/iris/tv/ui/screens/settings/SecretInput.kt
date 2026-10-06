package studio.kahn.iris.tv.ui.screens.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.components.focusRing
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * A password field: [studio.kahn.iris.tv.ui.components.TextInput]'s look,
 * its characters masked. (TextInput takes no visual transformation; this
 * belongs there once the shared components open again.)
 */
@Composable
fun SecretInput(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    keyboardActions: KeyboardActions = KeyboardActions.Default,
    imeAction: ImeAction = ImeAction.Done,
) {
    val source = remember { MutableInteractionSource() }
    val focused by source.collectIsFocusedAsState()
    val style = IrisType.control.copy(fontSize = 16.sp, lineHeight = 20.sp)
    BasicTextField(
        value = value,
        onValueChange = onValueChange,
        modifier = modifier.semantics { contentDescription = label },
        singleLine = true,
        textStyle = style.copy(color = IrisColor.ink),
        cursorBrush = SolidColor(IrisColor.accent),
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = imeAction),
        keyboardActions = keyboardActions,
        interactionSource = source,
        decorationBox = { inner ->
            Box(
                Modifier
                    .focusRing(focused, IrisShape.card)
                    .heightIn(min = IrisSize.controlLarge)
                    .background(IrisColor.surface, IrisShape.card)
                    .border(1.dp, IrisColor.line, IrisShape.card)
                    .padding(horizontal = 12.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                if (value.isEmpty()) {
                    Text(label, style = style.copy(fontSize = 14.sp), color = IrisColor.inkMuted, maxLines = 1)
                }
                inner()
            }
        },
    )
}
