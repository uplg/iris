package studio.kahn.iris.tv.ui.screens.player

import androidx.compose.runtime.Immutable
import studio.kahn.iris.tv.data.AvailableEpisodeEntry
import studio.kahn.iris.tv.data.EpisodeEntry
import studio.kahn.iris.tv.data.FileEntry
import studio.kahn.iris.tv.data.FileProgressEntry
import studio.kahn.iris.tv.ui.format.episodeCode
import studio.kahn.iris.tv.ui.format.formatSize

/** One row of the episodes side panel: an episode (whichever file holds it) or a file of the torrent. */
@Immutable
data class SideRow(
    val key: String,
    val infohash: String,
    val fileIdx: Int,
    /** "S2:E1 · Name" for an episode, the file name for a raw file. */
    val primary: String,
    /** The language (episodes) or the size (files). */
    val secondary: String,
    val mono: Boolean,
    val watched: Boolean,
    /** 0..100 when started. */
    val watchedPct: Double?,
    val active: Boolean,
    /** Discovered, not downloaded yet: the row offers "Grab and play". */
    val grab: GrabTarget? = null,
    val season: Long? = null,
    val episode: Long? = null,
) {
    val started: Boolean get() = !watched && watchedPct != null && watchedPct > 0
}

@Immutable
data class GrabTarget(val season: Long, val episode: Long, val language: String?)

data class SideInput(
    val infohash: String,
    val fileIdx: Int,
    val isTvCollection: Boolean,
    val episodes: List<EpisodeEntry>,
    val available: List<AvailableEpisodeEntry>,
    val videoFiles: List<FileEntry>,
    val progressByFile: Map<Int, FileProgressEntry>,
    /** (season, episode) → episode name, when TMDB knows it. */
    val names: Map<Pair<Long, Long>, String> = emptyMap(),
)

fun watchedPctOf(p: FileProgressEntry?): Double? {
    val dur = p?.durationSeconds ?: return null
    return if (dur > 0) minOf(100.0, p.positionSeconds / dur * 100.0) else null
}

private fun known(l: String?): Boolean = !l.isNullOrBlank() && l != "unknown"

/** The language to prefer among an episode's offers: the playing file's, else the series' dominant owned language. */
fun currentLanguage(current: EpisodeEntry?, episodes: List<EpisodeEntry>): String? {
    if (known(current?.language)) return current?.language
    return episodes.mapNotNull { it.language?.takeIf(::known) }
        .groupingBy { it }
        .eachCount()
        .maxByOrNull { it.value }
        ?.key
}

/** Whether the panel lists the collection's episodes (else the torrent's files). */
fun listsEpisodes(i: SideInput): Boolean = i.isTvCollection && (i.episodes.isNotEmpty() || i.available.isNotEmpty())

/**
 * The web's `sideRows`: a TV collection lists the current season's episodes
 * (every season while the playing file is not indexed yet), one row per
 * episode: the playing file, else the preferred language, else Multi, else the
 * first; an episode on disk never also shows a row to grab another language.
 * Anything else lists the torrent's video files.
 */
fun sideRows(i: SideInput): List<SideRow> {
    if (!listsEpisodes(i)) {
        return i.videoFiles.map { f ->
            val prog = i.progressByFile[f.index]
            SideRow(
                key = "f:${f.index}",
                infohash = i.infohash,
                fileIdx = f.index,
                primary = f.path.substringAfterLast('/'),
                secondary = formatSize(f.sizeBytes),
                mono = true,
                watched = prog?.completed == true,
                watchedPct = watchedPctOf(prog),
                active = f.index == i.fileIdx,
            )
        }
    }
    val current = i.episodes.firstOrNull { it.infohash == i.infohash && it.fileIdx.toInt() == i.fileIdx }
    val season = current?.season
    val lang = currentLanguage(current, i.episodes)
    fun primary(s: Long, e: Long): String =
        listOfNotNull(episodeCode(s, e), i.names[s to e]).joinToString(" · ")

    val owned = i.episodes.filter { season == null || it.season == season }.groupBy { it.season to it.episode }
    val downloaded = owned.values.map { variants ->
        val e = variants.firstOrNull { it.infohash == i.infohash && it.fileIdx.toInt() == i.fileIdx }
            ?: lang?.let { l -> variants.firstOrNull { it.language == l } }
            ?: variants.firstOrNull { it.language == "multi" }
            ?: variants.first()
        val prog = if (e.infohash == i.infohash) i.progressByFile[e.fileIdx.toInt()] else null
        SideRow(
            key = "dl:${e.infohash}:${e.fileIdx}",
            infohash = e.infohash,
            fileIdx = e.fileIdx.toInt(),
            primary = primary(e.season, e.episode),
            secondary = e.language?.takeIf(::known).orEmpty(),
            mono = false,
            watched = e.watched || prog?.completed == true,
            watchedPct = watchedPctOf(prog),
            active = e.infohash == i.infohash && e.fileIdx.toInt() == i.fileIdx,
            season = e.season,
            episode = e.episode,
        )
    }
    val discovered = i.available
        .filter { (season == null || it.season == season) && (it.season to it.episode) !in owned }
        .groupBy { it.season to it.episode }
        .values
        .map { variants ->
            val a = lang?.let { l -> variants.firstOrNull { it.language == l } }
                ?: variants.firstOrNull { it.language == "multi" }
                ?: variants.first()
            SideRow(
                key = "av:${a.season}:${a.episode}",
                infohash = "",
                fileIdx = -1,
                primary = primary(a.season, a.episode),
                secondary = a.language?.takeIf(::known).orEmpty(),
                mono = false,
                watched = false,
                watchedPct = null,
                active = false,
                grab = GrabTarget(a.season, a.episode, a.language),
                season = a.season,
                episode = a.episode,
            )
        }
    return (downloaded + discovered).sortedBy { (it.season ?: 0) * 100_000 + (it.episode ?: 0) }
}
