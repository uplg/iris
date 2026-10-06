package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Icon
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * A one-line text field (the search field, the server URL). Foundation
 * [BasicTextField], so it takes the D-pad on a TV and the soft keyboard on
 * a phone alike. [label] names it for accessibility and shows as the
 * placeholder while empty. Focused: the accent ring. [masked]: a password, its characters
 * hidden and the keyboard a password one.
 */
@Composable
fun TextInput(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    leadingIcon: ImageVector? = null,
    textStyle: TextStyle = IrisType.input,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
    keyboardActions: KeyboardActions = KeyboardActions.Default,
    enabled: Boolean = true,
    masked: Boolean = false,
) {
    val source = remember { MutableInteractionSource() }
    val focused by source.collectIsFocusedAsState()
    BasicTextField(
        value = value,
        onValueChange = onValueChange,
        modifier = modifier.semantics { contentDescription = label },
        enabled = enabled,
        singleLine = true,
        textStyle = textStyle.copy(color = IrisColor.ink),
        cursorBrush = SolidColor(IrisColor.accent),
        visualTransformation = if (masked) PasswordVisualTransformation() else VisualTransformation.None,
        keyboardOptions = if (masked) keyboardOptions.copy(keyboardType = KeyboardType.Password) else keyboardOptions,
        keyboardActions = keyboardActions,
        interactionSource = source,
        decorationBox = { inner ->
            Row(
                Modifier
                    .focusRing(focused, IrisShape.card)
                    .heightIn(min = IrisSize.controlLarge)
                    .background(IrisColor.surface, IrisShape.card)
                    .border(1.dp, IrisColor.line, IrisShape.card)
                    .padding(horizontal = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (leadingIcon != null) {
                    Icon(leadingIcon, contentDescription = null, tint = IrisColor.inkMuted, modifier = Modifier.size(IrisSize.icon))
                }
                Box(Modifier.weight(1f)) {
                    if (value.isEmpty()) {
                        Text(label, style = textStyle.copy(fontSize = IrisType.inputHint.fontSize), color = IrisColor.inkMuted, maxLines = 1)
                    }
                    inner()
                }
            }
        },
    )
}
