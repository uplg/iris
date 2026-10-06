package studio.kahn.iris.tv.ui.screens.home

import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import java.util.Locale
import kotlin.math.roundToInt
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.HomeSummary
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.data.SubtitlePick
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.ui.formatSize

/*
 * The words the home and discover screens say about what they read, as the web app says
 * them (`@iris/api/format`, `web/src/lib/home/data.ts`): one wording on both clients.
 */

/** A playback position as a clock: `32:10`, `1:02:03`. */
fun clock(seconds: Double): String {
    val total = seconds.coerceAtLeast(0.0).toLong()
    val h = total / 3600
    val m = (total % 3600) / 60
    val s = total % 60
    return if (h > 0) {
        "$h:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}"
    } else {
        "$m:${s.toString().padStart(2, '0')}"
    }
}

/** A length as people say it: `55 min`, `1 h 12 min`, `45 s` under a minute. */
fun duration(seconds: Double): String {
    val total = seconds.coerceAtLeast(0.0).roundToInt()
    if (total < 60) return "$total s"
    val h = total / 3600
    val m = ((total % 3600) / 60.0).roundToInt()
    if (h == 0) return "$m min"
    return if (m == 0) "$h h" else "$h h $m min"
}

/** What is left to watch: `23 min left`. */
fun timeLeft(seconds: Double): String = "${duration(seconds)} left"

/** `S2:E4`; a season alone: `Season 2`; an episode alone: `E19`. */
fun episodeCode(season: Long?, episode: Long?): String? = when {
    season == null -> episode?.let { "E$it" }
    episode == null || episode == 0L -> "Season $season"
    else -> "S$season:E$episode"
}

/** `S2:E5`, or "the next episode" when the server does not know which. */
fun nextName(item: ContinueWatchingItem): String = episodeCode(item.season, item.episode) ?: "the next episode"

/** `Movie`, `Series`, `Anime · Series` (no kind reads as a movie). */
fun kindLabel(kind: MediaKind?, anime: Boolean = false): String {
    val word = if (kind == MediaKind.tv) "Series" else "Movie"
    return if (anime) "Anime · $word" else word
}

/** A count with its noun: `1 download`, `3 downloads`. */
fun plural(n: Long, one: String, many: String = "${one}s"): String = "$n ${if (n == 1L) one else many}"

/** A share 0 to 100 as people read it: `42%`. */
fun percent(share: Double): String = "${share.roundToInt()}%"

// Release tokens that end the human title in a SCENE name (web `prettySceneName`).
private val SCENE_STOP = Regex(
    "^(19\\d{2}|20\\d{2}|s\\d{1,2}(e\\d{1,3})?|e\\d{1,3}|\\d{3,4}p|web|web-?dl|webrip|bluray|blu-?ray|brrip|bdrip|hdtv|" +
        "dvdrip|dvd|remux|x264|x265|h264|h265|hevc|avc|xvid|divx|aac\\d?|ac3|eac3|dts(-?hd)?(-?ma)?|ddp?\\d?|truehd|atmos|" +
        "flac|multi|vff|vfi|vof|vfq|vostfr|vost|vo|vf|french|truefrench|english|hdr|hdr10\\+?|dovi|dv|10bit|8bit|repack|" +
        "proper|internal|limited|uncut|unrated|extended|imax|complete|integrale)$",
    RegexOption.IGNORE_CASE,
)
private val SCENE_EXT = Regex("\\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv|srt|nfo)$", RegexOption.IGNORE_CASE)
private val YEAR = Regex("^(19|20)\\d{2}$")

/** A raw release name made readable when no trusted title is known: `Mercato (2025)`. */
fun prettySceneName(raw: String): String {
    val tokens = raw.replace(SCENE_EXT, "").split(Regex("[._\\s]+")).filter { it.isNotEmpty() }
    val title = mutableListOf<String>()
    var year: String? = null
    for (t in tokens) {
        if (SCENE_STOP.matches(t)) {
            if (YEAR.matches(t) && title.isNotEmpty()) year = t
            break
        }
        title += t
    }
    if (title.isEmpty()) return tokens.joinToString(" ")
    val name = title.joinToString(" ")
    return if (year != null) "$name ($year)" else name
}

/** The home's "Right now" line: only what is happening. */
fun rightNow(s: HomeSummary): List<String> = buildList {
    if (s.downloading > 0) {
        val eta = s.downloadingEtaSeconds?.takeIf { it > 0 }?.let { " · ${timeLeft(it.toDouble())}" }.orEmpty()
        add("${plural(s.downloading.toLong(), "download")} · ${percent(s.downloadingPct)}$eta")
    }
    if (s.newEpisodes > 0) add("${plural(s.newEpisodes, "new episode")} on your watchlist")
    s.disk?.let { add("${formatSize(it.freeBytes)} free on disk") }
    if (s.seeding > 0) add("Seeding ${plural(s.seeding.toLong(), "release")}")
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

/** `fr`, `fre`, `fr-FR` → "French"; an unknown code in capitals; absent → null. */
@OptIn(UnstableApi::class)
fun languageName(tag: String?): String? {
    val code = SubtitlePick.normalizeLang(tag) ?: return null
    val name = runCatching { Locale.forLanguageTag(code).getDisplayLanguage(Locale.ENGLISH) }.getOrNull()
    return if (name.isNullOrBlank() || name.equals(code, ignoreCase = true)) code.uppercase() else name
}

/** The languages a play will use, when the account (or the series) chose them. */
fun languagesLine(p: PlaybackPrefsResponse?): String? {
    if (p == null) return null
    val parts = buildList {
        p.audioLanguage?.takeIf { it.isNotBlank() }?.let { add("audio in ${languageName(it) ?: it}") }
        val subs = p.subtitleLanguage?.takeIf { it.isNotBlank() }
        when {
            subs == "off" -> add("subtitles off")
            subs != null -> add("subtitles in ${languageName(subs) ?: subs}")
        }
    }
    if (parts.isEmpty()) return null
    val series = if (p.forCollection == true) ", as chosen for this series" else ""
    return "Plays with ${parts.joinToString(", ")}$series."
}
