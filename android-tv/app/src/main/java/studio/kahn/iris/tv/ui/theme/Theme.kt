package studio.kahn.iris.tv.ui.theme

import androidx.compose.runtime.Composable
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.darkColorScheme

private val IrisDark = darkColorScheme(
    primary = androidx.compose.ui.graphics.Color(0xFFC084FC),
    onPrimary = androidx.compose.ui.graphics.Color(0xFF0B0D12),
    secondary = androidx.compose.ui.graphics.Color(0xFFA78BFA),
    background = androidx.compose.ui.graphics.Color(0xFF0B0D12),
    surface = androidx.compose.ui.graphics.Color(0xFF18181B),
    onSurface = androidx.compose.ui.graphics.Color(0xFFF5F5F7),
    onBackground = androidx.compose.ui.graphics.Color(0xFFF5F5F7),
    surfaceVariant = androidx.compose.ui.graphics.Color(0xFF27272A),
    onSurfaceVariant = androidx.compose.ui.graphics.Color(0xFFA1A1AA),
)

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun IrisTvTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = IrisDark,
        content = content,
    )
}
