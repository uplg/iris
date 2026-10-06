package studio.kahn.iris.tv.ui.screens.home

import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.HomeSummary
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.ui.format.episodeCode
import studio.kahn.iris.tv.ui.format.formatSize
import studio.kahn.iris.tv.ui.format.NO_SUBTITLES
import studio.kahn.iris.tv.ui.format.languageName
import studio.kahn.iris.tv.ui.format.percent
import studio.kahn.iris.tv.ui.format.plural
import studio.kahn.iris.tv.ui.format.timeLeft

/*
 * The words only the home and discover screens say about what they read, as the web app
 * says them (`web/src/lib/home/data.ts`); the shared ones are in `ui/format`.
 */

/** `S2:E5`, or "the next episode" when the server does not know which. */
fun nextName(item: ContinueWatchingItem): String = episodeCode(item.season, item.episode) ?: "the next episode"

/** The home's "Right now" line: only what is happening. */
fun rightNow(s: HomeSummary): List<String> = buildList {
    if (s.downloading > 0) {
        val eta = s.downloadingEtaSeconds?.takeIf { it > 0 }?.let { " · ${timeLeft(it.toDouble())}" }.orEmpty()
        add("${plural(s.downloading, "download")} · ${percent(s.downloadingPct)}$eta")
    }
    if (s.newEpisodes > 0) add("${plural(s.newEpisodes, "new episode")} on your watchlist")
    s.disk?.let { add("${formatSize(it.freeBytes)} free on disk") }
    if (s.seeding > 0) add("Seeding ${plural(s.seeding, "release")}")
}

/** What is still downloading in each collection, as a share of its bytes (0 to 100). */
fun downloadsByCollection(list: List<TorrentView>): Map<String, Double> {
    val sums = LinkedHashMap<String, Pair<Long, Long>>()
    for (t in list) {
        val id = t.collectionId?.toString() ?: continue
        if (t.finished) continue
        val (done, total) = sums[id] ?: (0L to 0L)
        sums[id] = (done + t.progressBytes) to (total + t.totalSizeBytes)
    }
    return sums.mapValues { (_, s) -> if (s.second > 0) s.first * 100.0 / s.second else 0.0 }
}

/** A release still fetching data (web `moving`): polls go quick while one does. */
fun isMoving(t: TorrentView): Boolean =
    !t.finished && t.progressPct < 100 && (t.state == TorrentState.live || t.state == TorrentState.initializing)

/** Seconds left to watch, when the length is known. */
fun secondsLeft(item: ContinueWatchingItem): Double? =
    item.durationSeconds?.takeIf { it > 0 }?.let { (it - item.positionSeconds).coerceAtLeast(0.0) }

/** Watched so far, 0 to 1, when the length is known. */
fun watchedShare(item: ContinueWatchingItem): Float? =
    item.durationSeconds?.takeIf { it > 0 }?.let { (item.positionSeconds / it).toFloat().coerceIn(0f, 1f) }

/** A resume, not a fresh start: there is a position worth keeping. */
fun isResuming(item: ContinueWatchingItem): Boolean = !item.grabbable && !item.nextUp && item.positionSeconds >= 5

/** The languages a play will use, when the account (or the series) chose them. */
fun languagesLine(p: PlaybackPrefsResponse?): String? {
    if (p == null) return null
    val parts = buildList {
        p.audioLanguage?.takeIf { it.isNotBlank() }?.let { add("audio in ${languageName(it) ?: it}") }
        val subs = p.subtitleLanguage?.takeIf { it.isNotBlank() }
        when {
            subs == NO_SUBTITLES -> add("subtitles off")
            subs != null -> add("subtitles in ${languageName(subs) ?: subs}")
        }
    }
    if (parts.isEmpty()) return null
    val series = if (p.forCollection == true) ", as chosen for this series" else ""
    return "Plays with ${parts.joinToString(", ")}$series."
}
