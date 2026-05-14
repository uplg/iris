package studio.kahn.iris.tv.data

import android.content.res.Resources
import android.util.Log
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.util.UnstableApi
import androidx.media3.common.util.Util
import androidx.media3.ui.DefaultTrackNameProvider
import androidx.media3.ui.PlayerControlView
import androidx.media3.ui.PlayerView
import androidx.media3.ui.TrackNameProvider
import java.util.Locale

/**
 * Custom track-name provider for the native Media3 selector. Two goals:
 *
 *  * Surface the `Forced` selection flag — [DefaultTrackNameProvider]
 *    ignores it entirely, so a regular and a forced French sub render
 *    as the same string.
 *  * Keep subtitle names short. The default builds names like
 *    "Anglais, Sous-titres" (or longer on SDH+forced combos), and on
 *    TV the popup is anchored to the gear which doesn't leave much
 *    room before the row gets clipped at the screen edge. We compress
 *    "Closed captions" to "SDH" and join with thin separators.
 *
 * Audio/video tracks still go through the default — their existing
 * "5.1", resolution + bitrate strings are already concise.
 */
@UnstableApi
internal class IrisTrackNameProvider(resources: Resources) : TrackNameProvider {
    private val delegate = DefaultTrackNameProvider(resources)

    override fun getTrackName(format: Format): String {
        if (MimeTypes.getTrackType(format.sampleMimeType) != C.TRACK_TYPE_TEXT) {
            return delegate.getTrackName(format)
        }

        val parts = mutableListOf<String>()
        languageDisplayName(format.language)?.let(parts::add)
            ?: format.label?.takeIf { it.isNotBlank() }?.let(parts::add)
            ?: parts.add("Subtitles")

        val roles = format.roleFlags
        when {
            roles and (C.ROLE_FLAG_CAPTION or C.ROLE_FLAG_DESCRIBES_MUSIC_AND_SOUND) != 0 ->
                parts += "SDH"
            roles and C.ROLE_FLAG_COMMENTARY != 0 -> parts += "Commentary"
            roles and C.ROLE_FLAG_ALTERNATE != 0 -> parts += "Alternate"
        }
        if (format.selectionFlags and C.SELECTION_FLAG_FORCED != 0) parts += "Forced"

        return parts.joinToString(" · ")
    }

    private fun languageDisplayName(rawCode: String?): String? {
        if (rawCode.isNullOrBlank() || rawCode.equals("und", ignoreCase = true)) return null
        // Media3's util normalises ISO 639-2/T (`fre`, `eng`) to the
        // 2-letter form `Locale.forLanguageTag` understands. Recent
        // Media3 versions tightened the signature to return a
        // non-nullable `String`, so there's nothing to coalesce — the
        // worst case is the input echoed back unchanged.
        val normalized = Util.normalizeLanguageCode(rawCode)
        if (normalized == "und") return null
        val name = runCatching {
            Locale.forLanguageTag(normalized).getDisplayLanguage(Locale.ENGLISH)
        }.getOrNull()
        return when {
            name.isNullOrBlank() || name.equals(normalized, ignoreCase = true) ->
                rawCode.uppercase()
            else -> name.replaceFirstChar { it.uppercase() }
        }
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
