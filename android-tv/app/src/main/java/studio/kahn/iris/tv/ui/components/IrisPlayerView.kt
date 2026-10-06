@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package studio.kahn.iris.tv.ui.components

import android.app.PendingIntent
import android.content.Context
import android.view.accessibility.CaptioningManager
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.getSystemService
import androidx.media3.common.Player
import androidx.media3.common.text.CueGroup
import androidx.media3.session.MediaSession
import androidx.media3.ui.SubtitleView
import androidx.media3.ui.compose.ContentFrame
import androidx.media3.ui.compose.SURFACE_TYPE_SURFACE_VIEW
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import studio.kahn.iris.tv.ui.theme.IrisColor

/**
 * The picture and its captions, without any controller (the player and Live
 * TV draw their own controls in Compose):
 *
 * - Media3's [ContentFrame] on a SurfaceView (the PlayerView default: the
 *   cheapest path and the one HDR passthrough takes), letterboxed to fit,
 *   the stage color until the first frame.
 * - A Media3 [SubtitleView] fed the player's cues, in the system caption
 *   style ([applySystemCaptionStyle]), re-applied when the person changes
 *   the caption settings. [liftCues] raises them above the controls.
 * - The screen stays on while it is shown (as PlayerView's `keepScreenOn`).
 */
@Composable
fun PlayerStage(
    player: Player?,
    modifier: Modifier = Modifier,
    liftCues: Boolean = false,
) {
    val hostView = LocalView.current
    DisposableEffect(hostView) {
        stagesShown.incrementAndGet()
        hostView.keepScreenOn = true
        // The next episode's stage composes before the previous one leaves.
        onDispose { if (stagesShown.decrementAndGet() == 0) hostView.keepScreenOn = false }
    }
    Box(modifier.fillMaxSize().background(IrisColor.stage), contentAlignment = Alignment.Center) {
        ContentFrame(
            player = player,
            modifier = Modifier.fillMaxSize(),
            surfaceType = SURFACE_TYPE_SURFACE_VIEW,
            contentScale = ContentScale.Fit,
            shutter = { Box(Modifier.fillMaxSize().background(IrisColor.stage)) },
        )
        SubtitleLayer(player, liftCues)
    }
}

@Composable
private fun SubtitleLayer(player: Player?, lifted: Boolean) {
    val context = LocalContext.current
    var view by remember { mutableStateOf<SubtitleView?>(null) }
    AndroidView(
        modifier = Modifier.fillMaxSize(),
        factory = { ctx -> SubtitleView(ctx).apply { applySystemCaptionStyle() } },
        update = { v ->
            view = v
            v.setBottomPaddingFraction(if (lifted) LIFTED_CUES_FRACTION else SubtitleView.DEFAULT_BOTTOM_PADDING_FRACTION)
        },
    )
    val subtitleView = view
    DisposableEffect(player, subtitleView) {
        if (player == null || subtitleView == null) return@DisposableEffect onDispose {}
        subtitleView.setCues(player.currentCues.cues)
        val listener = object : Player.Listener {
            override fun onCues(cueGroup: CueGroup) {
                subtitleView.setCues(cueGroup.cues)
            }
        }
        player.addListener(listener)
        onDispose {
            player.removeListener(listener)
            subtitleView.setCues(null)
        }
    }
    DisposableEffect(context, subtitleView) {
        val captioning = context.getSystemService<CaptioningManager>()
        if (subtitleView == null || captioning == null) return@DisposableEffect onDispose {}
        val listener = object : CaptioningManager.CaptioningChangeListener() {
            override fun onUserStyleChanged(userStyle: CaptioningManager.CaptionStyle) = subtitleView.applySystemCaptionStyle()
            override fun onFontScaleChanged(fontScale: Float) = subtitleView.applySystemCaptionStyle()
            override fun onEnabledChanged(enabled: Boolean) = subtitleView.applySystemCaptionStyle()
        }
        captioning.addCaptioningChangeListener(listener)
        onDispose { captioning.removeCaptioningChangeListener(listener) }
    }
}

/**
 * The person's caption settings (Android Settings → Accessibility →
 * Captions): `setUserDefaultStyle` reads `CaptioningManager.getUserStyle()`
 * through `CaptionStyleCompat.createFromCaptionStyle`, `setUserDefaultTextSize`
 * its font scale.
 */
private fun SubtitleView.applySystemCaptionStyle() {
    setUserDefaultStyle()
    setUserDefaultTextSize()
}

/** Cues sit above the bottom controls while they show. */
private const val LIFTED_CUES_FRACTION = 0.3f

private val sessionIds = AtomicLong()
private val stagesShown = AtomicInteger()

/**
 * A [MediaSession] for an in-activity player: remote media keys (play,
 * pause, fast-forward, rewind from Bluetooth or HDMI-CEC remotes), the
 * Assistant and the system's now-playing card reach [player]. No service: it
 * lives with the screen. Release it BEFORE the player. Ids are unique per
 * session because the next episode's screen builds its own while the
 * previous one is still leaving.
 */
fun buildMediaSession(context: Context, player: Player, tag: String): MediaSession {
    val app = context.applicationContext
    val builder = MediaSession.Builder(app, player).setId("iris-$tag-${sessionIds.incrementAndGet()}")
    val launch = app.packageManager.getLeanbackLaunchIntentForPackage(app.packageName)
        ?: app.packageManager.getLaunchIntentForPackage(app.packageName)
    if (launch != null) {
        builder.setSessionActivity(
            PendingIntent.getActivity(app, 0, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT),
        )
    }
    return builder.build()
}
