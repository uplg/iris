package studio.kahn.iris.tv.ui.screens.library

import studio.kahn.iris.tv.ui.format.AgoStyle
import studio.kahn.iris.tv.ui.format.ago
import androidx.compose.runtime.Immutable
import java.time.Instant
import java.time.ZoneId
import kotlin.math.max
import kotlin.math.min
import studio.kahn.iris.tv.data.HistoryItem
import studio.kahn.iris.tv.ui.format.clock
import studio.kahn.iris.tv.ui.format.episodeCode
import studio.kahn.iris.tv.ui.format.percent

// A watch history grouped by what was watched (web `lib/history/groups.ts` and `words.ts`):
// a series under its title with its episodes, a film on one line. A title whose files were all
// reclaimed stays listed (a ghost), with the way back to it.

@Immutable
data class HistoryGroup(
    val key: String,
    val collectionId: String?,
    val title: String,
    /** The first verified TMDB poster among its rows. */
    val posterPath: String?,
    /** Every file of it is gone from disk. */
    val ghost: Boolean,
    /** One file with no episode to it (a film): drawn as one line. */
    val solo: Boolean,
    val items: List<HistoryItem>,
)

fun historyKey(it: HistoryItem) = "${it.infohash}:${it.fileIdx}"

/** The groups, in the order of each one's latest watch (the server sends newest first). */
fun groupHistory(items: List<HistoryItem>): List<HistoryGroup> {
    val order = LinkedHashMap<String, MutableList<HistoryItem>>()
    for (it in items) order.getOrPut(it.collectionId?.toString() ?: "solo:${it.infohash}") { mutableListOf() } += it
    return order.map { (key, rows) ->
        val first = rows.first()
        HistoryGroup(
            key = key,
            collectionId = first.collectionId?.toString(),
            title = first.collectionTitle ?: first.torrentName,
            posterPath = rows.firstNotNullOfOrNull { it.posterPath },
            ghost = rows.all { it.deleted },
            solo = rows.size == 1 && first.season == null && first.absoluteEpisode == null,
            items = rows,
        )
    }
}

/** Gone from disk, and its release is known: it can be downloaded again (the position applies again). */
fun canRestore(it: HistoryItem) = it.deleted && it.sourceProvider != null && it.sourceExternalId != null

/** `Watched to the end`, `42% watched, stopped at 32:10`, `Not started`. */
fun progressWords(position: Double, total: Double?, completed: Boolean): String = when {
    completed -> "Watched to the end"
    position < 1 -> "Not started"
    total == null || total <= 0 -> "Stopped at ${clock(position)}"
    else -> "${percent(min(100.0, position / total * 100))} watched, stopped at ${clock(position)}"
}

fun watchedShare(position: Double, total: Double?, completed: Boolean): Float = when {
    completed -> 1f
    total != null && total > 0 -> min(1.0, max(0.0, position / total)).toFloat()
    else -> 0f
}

/** What exactly was watched: `E1156`, `S2:E4`, `Season 2`, else the file's name. */
fun whatWatched(it: HistoryItem): String? = when {
    it.absoluteEpisode != null -> episodeCode(null, it.absoluteEpisode)
    it.season != null -> episodeCode(it.season, it.episode)
    else -> fileName(it.filePath)
}

/** A line's label: the title for a film, what was watched for an episode. */
fun historyLabel(group: HistoryGroup, it: HistoryItem): String =
    if (group.solo) group.title else whatWatched(it) ?: it.torrentName

fun historyFacts(it: HistoryItem, now: Instant = Instant.now(), zone: ZoneId = ZoneId.systemDefault()): String =
    "${progressWords(it.positionSeconds, it.durationSeconds, it.completed)} · Last watched ${ago(it.lastWatchedAt, AgoStyle.Sentence, now, zone)}"
