package studio.kahn.iris.tv.ui.components

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ActivityInfo
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalContext
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

/**
 * Playback window mode, held while the calling composable is in
 * composition and undone on leave:
 *
 *   * sensor-landscape orientation — video always plays landscape on a
 *     phone while the browsing UI is free to rotate;
 *   * immersive system bars (swipe to peek) — this also zeroes the
 *     `safeDrawing` insets, so IrisRoot's global safe-zone padding
 *     collapses and the video is genuinely full-bleed.
 *
 * On TV the device is landscape-only with no system bars: no-op.
 */
@Composable
fun LockLandscape() {
    val context = LocalContext.current
    DisposableEffect(Unit) {
        val activity = context.findActivity()
        val previousOrientation = activity?.requestedOrientation
        activity?.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        val controller = activity?.window?.let { w ->
            WindowCompat.getInsetsController(w, w.decorView)
        }
        controller?.systemBarsBehavior =
            WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        controller?.hide(WindowInsetsCompat.Type.systemBars())
        // Draw INTO the display cutout (Netflix/YouTube behaviour) — without
        // this the notch side keeps a dead band in landscape playback.
        var previousCutoutMode = 0
        if (android.os.Build.VERSION.SDK_INT >= 28) {
            activity?.window?.attributes?.let { attrs ->
                previousCutoutMode = attrs.layoutInDisplayCutoutMode
                attrs.layoutInDisplayCutoutMode =
                    android.view.WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
                activity.window.attributes = attrs
            }
        }
        onDispose {
            controller?.show(WindowInsetsCompat.Type.systemBars())
            if (android.os.Build.VERSION.SDK_INT >= 28) {
                activity?.window?.attributes?.let { attrs ->
                    attrs.layoutInDisplayCutoutMode = previousCutoutMode
                    activity.window.attributes = attrs
                }
            }
            activity?.requestedOrientation =
                previousOrientation ?: ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
        }
    }
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
