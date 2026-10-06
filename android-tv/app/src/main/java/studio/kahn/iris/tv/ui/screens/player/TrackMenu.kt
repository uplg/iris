@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package studio.kahn.iris.tv.ui.screens.player

import studio.kahn.iris.tv.ui.format.SUBTITLES_OFF
import androidx.compose.runtime.Immutable
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.Player
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.Tracks
import studio.kahn.iris.tv.data.ForcedTextTracks
import studio.kahn.iris.tv.data.MediaProbe

/** One radio option of the audio and subtitles panel. [id]: `a:<n>`, `s:<n>` or [SUBTITLES_OFF_ID]. */
@Immutable
data class TrackChoice(val id: String, val label: String, val selected: Boolean)

/** What the panel and the bottom bar's summary show. */
@Immutable
data class TrackMenu(
    val audio: List<TrackChoice>,
    /** [SUBTITLES_OFF] first; empty when the file has no subtitles at all. */
    val subtitles: List<TrackChoice>,
    val summary: String,
) {
    val isEmpty: Boolean get() = audio.size <= 1 && subtitles.size <= 1

    companion object {
        val Empty = TrackMenu(emptyList(), emptyList(), "")
    }
}

const val SUBTITLES_OFF_ID = "s:off"

/**
 * The panel's options from the player's tracks, named by [TrackNaming]. The
 * N-th group of a type is the N-th probed stream on the direct path, and the
 * N-th TEXT-based one on the server's HLS (only those are side-loaded), so
 * names come from the probe when the counts agree, else from Media3's format.
 * Tracks the device cannot play are left out, as the native menu did.
 */
fun trackMenu(tracks: Tracks, probe: MediaProbe?, route: PlayRoute, forced: ForcedTextTracks? = null): TrackMenu {
    val audioGroups = tracks.groups.filter { it.type == C.TRACK_TYPE_AUDIO }
    val subGroups = tracks.groups.filter { it.type == C.TRACK_TYPE_TEXT }
    if (audioGroups.isEmpty() && subGroups.isEmpty()) return TrackMenu.Empty

    val probedAudio = probe?.audio?.takeIf { it.size == audioGroups.size }
    val audioInfos = audioGroups.mapIndexed { i, g ->
        probedAudio?.get(i)?.let { AudioTrackInfo(it.language, it.title, it.channels, it.codec) }
            ?: g.getTrackFormat(0).audioInfo()
    }
    val probedSubs = probe?.let { RouteTracks.of(it, route).subtitles }
        ?.takeIf { it.size == subGroups.size }
    val subInfos = subGroups.mapIndexed { i, g ->
        probedSubs?.get(i)?.let { SubtitleTrackInfo(it.language, it.title, it.forced, it.textBased) }
            ?: g.getTrackFormat(0).subtitleInfo(forced)
    }
    val audioNames = TrackNaming.audioLabels(audioInfos)
    val subNames = TrackNaming.subtitleLabels(subInfos)

    val audio = audioGroups.indices
        .filter { audioGroups[it].isSupported }
        .map { i -> TrackChoice("a:$i", audioNames[i], audioGroups[i].isSelected) }
    val selectedSub = subGroups.indexOfFirst { it.isSelected }
    val playable = subGroups.indices.filter { subGroups[it].isSupported }
    val subtitles = if (playable.isEmpty()) {
        emptyList()
    } else {
        listOf(TrackChoice(SUBTITLES_OFF_ID, SUBTITLES_OFF, selectedSub < 0)) +
            playable.map { i -> TrackChoice("s:$i", subNames[i], subGroups[i].isSelected) }
    }
    val selectedAudio = audioGroups.indexOfFirst { it.isSelected }
    return TrackMenu(
        audio = audio,
        subtitles = subtitles,
        summary = TrackNaming.summary(audioInfos.getOrNull(selectedAudio), subInfos.getOrNull(selectedSub), hasSubtitles = playable.isNotEmpty()),
    )
}

/**
 * Plays the chosen track, the way the native menu did: an override on that
 * group with its type enabled, or [SUBTITLES_OFF] turning text off. The engine's
 * onTracksChanged then saves the pick.
 */
fun Player.choose(tracks: Tracks, id: String) {
    val params = trackSelectionParameters.buildUpon()
    when {
        id == SUBTITLES_OFF_ID -> params.clearOverridesOfType(C.TRACK_TYPE_TEXT).setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
        id.startsWith("a:") || id.startsWith("s:") -> {
            val type = if (id.startsWith("a:")) C.TRACK_TYPE_AUDIO else C.TRACK_TYPE_TEXT
            val group = tracks.groups.filter { it.type == type }.getOrNull(id.substring(2).toIntOrNull() ?: -1) ?: return
            params.setOverrideForType(TrackSelectionOverride(group.mediaTrackGroup, 0)).setTrackTypeDisabled(type, false)
        }
        else -> return
    }
    trackSelectionParameters = params.build()
}

private fun Format.audioInfo() = AudioTrackInfo(
    language = language,
    title = label,
    channels = channelCount.coerceAtLeast(0),
    codec = sampleMimeType?.let(::mimeWord) ?: "audio",
)

private fun Format.subtitleInfo(forced: ForcedTextTracks?) = SubtitleTrackInfo(
    language = language,
    title = label,
    forced = selectionFlags and C.SELECTION_FLAG_FORCED != 0 || forced?.isForced(id) == true,
    textBased = sampleMimeType != MimeTypes.APPLICATION_PGS && sampleMimeType != MimeTypes.TEXT_SSA,
)

private fun mimeWord(mime: String): String = mime.substringAfter('/').removePrefix("x-").removePrefix("vnd.")
