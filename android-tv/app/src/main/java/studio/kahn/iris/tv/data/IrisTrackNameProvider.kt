package studio.kahn.iris.tv.data

import android.content.res.Resources
import android.util.Log
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.util.UnstableApi
import androidx.media3.ui.DefaultTrackNameProvider
import androidx.media3.ui.PlayerControlView
import androidx.media3.ui.PlayerView
import androidx.media3.ui.TrackNameProvider

/**
 * Wrap Media3's [DefaultTrackNameProvider] to surface the `Forced`
 * selection flag in the track-selection menu. The default provider
 * builds names from `Format.language` + role flags only — it ignores
 * `SELECTION_FLAG_FORCED` entirely, so a regular and a forced French
 * sub track both render as "French".
 *
 * Closed-captions / SDH already get a built-in "Closed captions" suffix
 * when the role flags include `CAPTION` / `DESCRIBES_MUSIC_AND_SOUND`,
 * which is what we set on the [androidx.media3.common.MediaItem.SubtitleConfiguration]
 * for SDH-flagged tracks. We don't double up on that here.
 */
@UnstableApi
internal class IrisTrackNameProvider(resources: Resources) : TrackNameProvider {
    private val delegate = DefaultTrackNameProvider(resources)

    override fun getTrackName(format: Format): String {
        val base = delegate.getTrackName(format)
        val forced = (format.selectionFlags and C.SELECTION_FLAG_FORCED) != 0
        return if (forced) "$base · Forced" else base
    }
}

/**
 * Inject [IrisTrackNameProvider] into the [PlayerControlView] embedded
 * inside [PlayerView]. The Media3 1.10 API exposes no setter — the
 * provider is built once in the controller's constructor — so we
 * reflect the private `trackNameProvider` field. A `-keep` rule in
 * `proguard-rules.pro` pins the field name through R8.
 *
 * Silently no-ops if Media3's internals shift in a future bump; the
 * native menu just falls back to the upstream labels.
 */
@UnstableApi
fun installIrisTrackNameProvider(playerView: PlayerView) {
    runCatching {
        val controllerField = PlayerView::class.java.getDeclaredField("controller")
            .apply { isAccessible = true }
        val controller = controllerField.get(playerView) as? PlayerControlView ?: return
        val tnpField = PlayerControlView::class.java.getDeclaredField("trackNameProvider")
            .apply { isAccessible = true }
        tnpField.set(controller, IrisTrackNameProvider(playerView.resources))
    }.onFailure { Log.w("Iris", "TrackNameProvider swap failed: ${it.message}") }
}
