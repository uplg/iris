package studio.kahn.iris.tv.ui.screens.search

import java.time.Instant
import java.time.OffsetDateTime
import java.time.temporal.ChronoUnit
import java.util.Locale
import kotlin.math.roundToInt
import studio.kahn.iris.tv.data.AudioInfo
import studio.kahn.iris.tv.data.LibraryMatch
import studio.kahn.iris.tv.data.MediaInfoSummary
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.SubInfo
import studio.kahn.iris.tv.data.TitleCard
import studio.kahn.iris.tv.ui.formatSize

// A release said for people (web `search/release.ts` and `@iris/api/format`):
// what title it is, what part of it, how it sounds and looks, whether the
// swarm can deliver it. Every search surface reads these, never its own.

/** The one key of a release across trackers (two trackers may reuse an id). */
fun releaseKey(r: SearchResult): String = "${r.providerId}:${r.externalId}"

fun plural(n: Int, one: String, many: String = "${one}s"): String = "$n ${if (n == 1) one else many}"

fun kindWord(kind: String?): String? = when (kind) {
    "tv" -> "Series"
    "movie" -> "Movie"
    else -> null
}

fun kindWord(kind: MediaKind?): String? = kindWord(kind?.value)

/** `S2:E4`; a season alone: `Season 2`. */
fun episodeCode(season: Int?, episode: Int?): String? = when {
    season == null -> episode?.let { "E$it" }
    episode == null || episode == 0 -> "Season $season"
    else -> "S$season:E$episode"
}

/** A season or an episode as people say it; episode 0 is the parser's whole-season mark. */
fun partWords(season: Int?, episode: Int?, name: String = ""): String? = when {
    season == null -> if (COMPLETE.containsMatchIn(name)) "Complete series" else null
    episode == null || episode == 0 -> "Season $season, complete"
    else -> episodeCode(season, episode)
}

private val COMPLETE = Regex("""\b(complete|integrale|int[ée]grale)\b""", RegexOption.IGNORE_CASE)

private val SCENE_STOP = Regex(
    "^(19\\d{2}|20\\d{2}|s\\d{1,2}(e\\d{1,3})?|e\\d{1,3}|\\d{3,4}p|web|web-?dl|webrip|bluray|blu-?ray|brrip|bdrip|hdtv|" +
        "dvdrip|dvd|remux|x264|x265|h264|h265|hevc|avc|xvid|divx|aac\\d?|ac3|eac3|dts(-?hd)?(-?ma)?|ddp?\\d?|truehd|" +
        "atmos|flac|multi|vff|vfi|vof|vfq|vostfr|vost|vo|vf|french|truefrench|english|hdr|hdr10\\+?|dovi|dv|10bit|8bit|" +
        "repack|proper|internal|limited|uncut|unrated|extended|imax|complete|integrale)$",
    RegexOption.IGNORE_CASE,
)
private val EXTENSION = Regex("""\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv|srt|nfo)$""", RegexOption.IGNORE_CASE)
private val YEAR = Regex("^(19|20)\\d{2}$")

/** A raw SCENE name cleaned for display when no TMDB title is known: "Mercato (2025)". */
fun prettySceneName(raw: String): String {
    val tokens = raw.replace(EXTENSION, "").split(Regex("[._\\s]+")).filter { it.isNotEmpty() }
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

/** The title a release belongs to, as a card names it. */
fun titleOf(r: SearchResult): String = r.titleMatch?.title ?: prettySceneName(r.title)

private val RESOLUTION = Regex("""\b(4320|2160|1440|1080|720|576|480)[pi]\b""", RegexOption.IGNORE_CASE)
private val UHD = Regex("""\b(4k|uhd)\b""", RegexOption.IGNORE_CASE)

/** The picture height a release name says (no field for it on the wire). */
fun resolution(name: String): String? =
    RESOLUTION.find(name)?.let { "${it.groupValues[1]}p" } ?: if (UHD.containsMatchIn(name)) "2160p" else null

fun codecWord(codec: String?): String? = when (codec) {
    "h264" -> "H.264"
    "hevc" -> "HEVC"
    "av1" -> "AV1"
    "vp9" -> "VP9"
    else -> null
}

fun seedersWords(n: Int?): String? = n?.let { if (it == 1) "1 seeder" else "${String.format(Locale.ENGLISH, "%,d", it)} seeders" }

/** A confirmed empty swarm: its pieces would never all arrive. Unknown is not dead. */
fun isDead(seeders: Int?): Boolean = seeders == 0

const val DEAD = "No seeders right now"

private val SHORT_LANGUAGE = mapOf("fr" to "French", "en" to "English", "multi" to "MULTI", "vost" to "VOSTFR", "vo" to "VO")

/** The grid's short "what" (TVSearchGrid): "Season 2 · MULTI", "S2:E7 · French", "Movie 2006 · VO". */
fun gridWhat(r: SearchResult): String? {
    val part = episodeCode(r.parsedSeason, r.parsedEpisode)
        ?: if ((r.titleMatch?.kind ?: r.kind) == MediaKind.movie) listOfNotNull("Movie", (r.titleMatch?.year ?: r.year)?.toString()).joinToString(" ") else null
    return listOfNotNull(part, SHORT_LANGUAGE[r.languageTag]).joinToString(" · ").ifEmpty { null }
}

/** "142 seeders · 12.4 GB · torr9 · 3d ago" */
fun factsLine(r: SearchResult, now: Instant = Instant.now()): String = listOfNotNull(
    seedersWords(r.seeders),
    r.sizeBytes?.let(::formatSize),
    r.providerId,
    r.uploadedAt?.let { formatRelative(it, now) },
).joinToString(" · ")

/** "today", "3d ago", "2mo ago", "1y ago" (web `formatRelative`). */
fun formatRelative(at: OffsetDateTime, now: Instant = Instant.now()): String {
    val days = ChronoUnit.DAYS.between(at.toInstant(), now)
    if (days < 1) return "today"
    if (days < 30) return "${days}d ago"
    val months = days / 30
    if (months < 12) return "${months}mo ago"
    return "${months / 12}y ago"
}

/** Already on disk, with the file to play: the release plays from there. */
data class OwnedFile(val infohash: String, val fileIdx: Int)

fun ownedFile(r: SearchResult): OwnedFile? {
    val infohash = r.libraryInfohash ?: return null
    val idx = r.libraryFileIdx ?: return null
    return if (r.alreadyInLibrary == true) OwnedFile(infohash, idx) else null
}

/** Where a library match leads: the exact episode asked for plays at once, else its collection. */
sealed interface MatchTarget {
    val action: String
    val facts: String

    data class Play(val infohash: String, val fileIdx: Int, override val action: String, override val facts: String) : MatchTarget
    data class Open(val collectionId: String, override val facts: String) : MatchTarget {
        override val action: String get() = "Open"
    }
}

fun matchTarget(m: LibraryMatch): MatchTarget {
    val infohash = m.episodeInfohash
    val idx = m.episodeFileIdx
    if (infohash != null && idx != null) {
        val code = episodeCode(m.episodeSeason?.toInt(), m.episodeNumber?.toInt())
        return MatchTarget.Play(infohash, idx.toInt(), "Play ${code.orEmpty()}".trim(), "The episode you asked for is on disk")
    }
    val seasonCount = m.seasonEpisodeCount
    val season = m.episodeSeason
    val facts = when {
        seasonCount != null && season != null -> "Season $season: ${plural(seasonCount.toInt(), "episode")} on disk"
        m.kind == "tv" -> "${plural(m.episodeCount.toInt(), "episode")} on disk"
        m.torrentCount > 1 -> "${plural(m.torrentCount.toInt(), "release")} on disk"
        else -> "On disk"
    }
    return MatchTarget.Open(m.collectionId, facts)
}

/** "Series · 2022" */
fun titleMeta(t: TitleCard): String = listOfNotNull(kindWord(t.kind), t.year?.toString()).joinToString(" · ")

/** A title card's state from what is known (the library, the loaded releases); null when nothing is. */
fun titleStatus(t: TitleCard, counts: Map<Long, Int>): String? {
    val n = counts[t.tmdbId] ?: 0
    val releases = if (n > 0) plural(n, "release") else null
    return if (t.collectionId != null) "In your library" else releases
}

private val CHANNELS = mapOf(1 to "mono", 2 to "stereo", 6 to "5.1", 8 to "7.1")

/** "55 min", "1 h 12 min", "45 s" (web `duration`). */
fun durationWords(seconds: Int): String {
    val total = seconds.coerceAtLeast(0)
    if (total < 60) return "$total s"
    val h = total / 3600
    val m = ((total % 3600) / 60.0).roundToInt()
    if (h == 0) return "$m min"
    return if (m == 0) "$h h" else "$h h $m min"
}

/** The tracker's technical sheet, one line for the picture. */
fun videoWords(mi: MediaInfoSummary?): String? {
    val v = mi?.video ?: return null
    return listOfNotNull(
        v.codec,
        v.resolution,
        v.hdr,
        v.fps?.let { "${(it * 100).roundToInt() / 100.0} fps".replace(".0 fps", " fps") },
        v.durationSecs?.takeIf { it > 0 }?.let(::durationWords),
    ).joinToString(" · ").ifEmpty { null }
}

private fun track(t: AudioInfo): String {
    val extra = listOfNotNull(t.commercialName ?: t.codec, t.channels?.let { CHANNELS[it] ?: "$it channels" })
    return (t.lang ?: "Unknown language") + if (extra.isEmpty()) "" else " (${extra.joinToString(", ")})"
}

fun audioWords(mi: MediaInfoSummary?): String? =
    mi?.audio?.takeIf { it.isNotEmpty() }?.joinToString(", ", transform = ::track)

private fun sub(s: SubInfo): String {
    val extra = listOfNotNull(
        s.format,
        if (s.forced == true) "forced" else null,
        if (s.title?.lowercase()?.contains("sdh") == true) "for the deaf and hard of hearing" else null,
    )
    return (s.lang ?: "Unknown language") + if (extra.isEmpty()) "" else " (${extra.joinToString(", ")})"
}

fun subtitleWords(mi: MediaInfoSummary?): String? =
    mi?.subtitles?.takeIf { it.isNotEmpty() }?.joinToString(", ", transform = ::sub)
