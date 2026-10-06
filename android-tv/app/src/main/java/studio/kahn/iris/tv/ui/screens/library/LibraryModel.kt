package studio.kahn.iris.tv.ui.screens.library

import androidx.compose.runtime.Immutable
import kotlin.math.max
import kotlin.math.min
import studio.kahn.iris.tv.data.CollectionListItem
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.HomeSummary
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.ui.format.duration
import studio.kahn.iris.tv.ui.format.episodeCode
import studio.kahn.iris.tv.ui.format.formatSize
import studio.kahn.iris.tv.ui.format.formatSpeed
import studio.kahn.iris.tv.ui.format.percent
import studio.kahn.iris.tv.ui.format.plural
import studio.kahn.iris.tv.ui.format.prettySceneName
import studio.kahn.iris.tv.ui.format.recentTime

// The library's facts in words, as the web's `lib/library/model.ts` says them.

/** What a state line means. Each is drawn with words and an icon, never color alone. */
enum class Tone { Ok, Busy, Warn, Info, Available }

@Immutable
data class Status(val tone: Tone, val text: String)

enum class TitleKind(val word: String, val filterLabel: String) {
    Movie("Movie", "Movies"),
    Series("Series", "Series"),
    Anime("Anime", "Anime"),
}

fun kindOf(c: CollectionListItem): TitleKind = when {
    c.isAnime == true -> TitleKind.Anime
    c.kind == MediaKind.tv -> TitleKind.Series
    else -> TitleKind.Movie
}

@Immutable
data class TitleCounts(val total: Int, val text: String)

/** `22 movies · 38 series · 4 anime`: what is on disk, ghosts left out. */
fun titleCounts(items: List<CollectionListItem>): TitleCounts {
    val live = items.filter { it.ghost != true }
    fun n(k: TitleKind) = live.count { kindOf(it) == k }
    return TitleCounts(
        live.size,
        listOf(plural(n(TitleKind.Movie), "movie"), "${n(TitleKind.Series)} series", "${n(TitleKind.Anime)} anime")
            .joinToString(" · "),
    )
}

/** What a title's releases are doing now. [played] is epoch ms, 0 if never played. */
@Immutable
data class Activity(val fetching: List<TorrentView> = emptyList(), val trouble: Boolean = false, val played: Long = 0)

fun activityByCollection(torrents: List<TorrentView>): Map<String, Activity> {
    val out = HashMap<String, Activity>()
    for (t in torrents) {
        val id = t.collectionId?.toString() ?: continue
        var a = out[id] ?: Activity()
        when (groupOf(t)) {
            ReleaseGroup.Downloading -> a = a.copy(fetching = a.fetching + t)
            ReleaseGroup.Attention -> if (!t.finished) a = a.copy(fetching = a.fetching + t, trouble = true)
            ReleaseGroup.Seeding -> Unit
        }
        t.lastPlayedAt?.let { a = a.copy(played = max(a.played, it.toInstant().toEpochMilli())) }
        out[id] = a
    }
    return out
}

/** Overall progress of some releases, by bytes (0 to 100). */
fun bytesPct(list: List<TorrentView>): Double {
    val total = list.sumOf { it.totalSizeBytes }
    return if (total > 0) list.sumOf { it.progressBytes } * 100.0 / total else 0.0
}

private val SEASON_IN_NAME = Regex("(?:^|[^a-z0-9])s(\\d{1,2})(?:e(\\d{1,3}))?(?![a-z0-9])", RegexOption.IGNORE_CASE)

/** The season or episode a release carries (`S4`, `S4:E2`), when its name says it. */
fun seasonOf(name: String?): String? {
    val m = SEASON_IN_NAME.find(name.orEmpty()) ?: return null
    val season = m.groupValues[1]
    val episode = m.groupValues[2].takeIf { it.isNotEmpty() }?.toLong()
    val code = episodeCode(season.toLong(), episode)
    return if (code?.startsWith("Season") == true) "S${season.removePrefix("0")}" else code
}

/** A title's facts: `Series · 4.2 GB`. */
fun titleMeta(c: CollectionListItem): String =
    if (c.ghost == true) kindOf(c).word else "${kindOf(c).word} · ${formatSize(c.totalSizeBytes)}"

/** A title's state in words: on disk, downloading, needs a hand, or gone. */
fun titleStatus(c: CollectionListItem, a: Activity?): Status {
    if (c.ghost == true) return Status(Tone.Info, "No longer on disk")
    if (a != null && a.fetching.isNotEmpty()) {
        val pct = percent(bytesPct(a.fetching))
        val what = if (a.fetching.size == 1) seasonOf(a.fetching[0].name) else plural(a.fetching.size, "release")
        if (a.trouble) return Status(Tone.Warn, "Download stuck · $pct")
        return Status(Tone.Busy, if (what != null) "Downloading $what · $pct" else "Downloading · $pct")
    }
    if (c.kind == MediaKind.tv && c.episodeCount > 0) return Status(Tone.Ok, "${plural(c.episodeCount, "episode")} on disk")
    if (c.torrentCount > 1) return Status(Tone.Ok, "${plural(c.torrentCount, "release")} on disk")
    return Status(Tone.Ok, "On disk")
}

enum class TypeFilter(val label: String, val kind: TitleKind?) {
    All("All", null),
    Movies("Movies", TitleKind.Movie),
    Series("Series", TitleKind.Series),
    Anime("Anime", TitleKind.Anime),
}

enum class ShowFilter { All, Downloading, Gone }

enum class Sort(val label: String, val words: String) {
    Recent("Recently added", "Sorted by recently added"),
    Watched("Recently watched", "Sorted by recently watched"),
    Title("Title A to Z", "Sorted by title"),
    Size("Size on disk", "Sorted by size on disk"),
}

@Immutable
data class TitleFilters(
    val query: String = "",
    val type: TypeFilter = TypeFilter.All,
    val show: ShowFilter = ShowFilter.All,
    val sort: Sort = Sort.Recent,
) {
    val filtered: Boolean get() = query.isNotBlank() || type != TypeFilter.All || show != ShowFilter.All
}

/** The titles shown: filtered, then sorted (`Recent` keeps the server's newest-first order). */
fun filterTitles(items: List<CollectionListItem>, f: TitleFilters, activity: Map<String, Activity>): List<CollectionListItem> {
    val q = f.query.trim().lowercase()
    val out = items.filter { c ->
        val id = c.id.toString()
        when {
            f.type.kind != null && kindOf(c) != f.type.kind -> false
            f.show == ShowFilter.Downloading && activity[id]?.fetching.isNullOrEmpty() -> false
            f.show == ShowFilter.Gone && c.ghost != true -> false
            q.isEmpty() -> true
            else -> c.displayTitle.lowercase().contains(q) || c.tmdbId?.toString()?.contains(q) == true
        }
    }
    return when (f.sort) {
        Sort.Recent -> out
        Sort.Title -> out.sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER) { it.displayTitle })
        Sort.Size -> out.sortedByDescending { it.totalSizeBytes }
        Sort.Watched -> out.sortedByDescending { activity[it.id.toString()]?.played ?: 0L }
    }
}

/** The "Show" choices worth offering: a state no title is in is not offered. */
fun showChoices(items: List<CollectionListItem>, activity: Map<String, Activity>): List<Pair<ShowFilter, String>> {
    val downloading = items.count { !activity[it.id.toString()]?.fetching.isNullOrEmpty() }
    val ghosts = items.count { it.ghost == true }
    return buildList {
        add(ShowFilter.All to "Everything")
        if (downloading > 0) add(ShowFilter.Downloading to "Downloading ($downloading)")
        if (ghosts > 0) add(ShowFilter.Gone to "No longer on disk ($ghosts)")
    }
}

/** `64 titles`, or `Showing 3 of 64 titles`. */
fun countWords(shown: Int, all: Int): String =
    if (shown == all) plural(all, "title") else "Showing $shown of $all titles"

enum class ReleaseGroup(val title: String) {
    Downloading("Downloading"),
    Attention("Needs attention"),
    Seeding("Seeding"),
}

private fun done(t: TorrentView) = t.finished || t.progressPct >= 100.0

/** A release still fetching data (not finished, not stopped): what makes the screens poll fast. */
fun moving(t: TorrentView): Boolean =
    !t.finished && t.progressPct < 100.0 && (t.state == TorrentState.live || t.state == TorrentState.initializing)

/** Fetching, needing a hand (an error, paused, no peers), or sharing what it has. */
fun groupOf(t: TorrentView): ReleaseGroup = when {
    t.state == TorrentState.error || t.state == TorrentState.paused -> ReleaseGroup.Attention
    done(t) -> ReleaseGroup.Seeding
    t.state == TorrentState.live && t.peers == 0 && t.downloadSpeedBps == 0L -> ReleaseGroup.Attention
    else -> ReleaseGroup.Downloading
}

/** Seconds until a release finishes at its current speed; null when nothing moves. */
fun etaSeconds(t: TorrentView): Double? {
    if (t.downloadSpeedBps <= 0) return null
    return max(0L, t.totalSizeBytes - t.progressBytes).toDouble() / t.downloadSpeedBps
}

fun ratioOf(sent: Long, received: Long?): Double? =
    if (received != null && received > 0) sent.toDouble() / received else null

/** A release's state line, in words. */
fun releaseStatus(t: TorrentView): Status {
    val pct = percent(min(100.0, max(0.0, t.progressPct)))
    if (t.state == TorrentState.error) {
        return Status(Tone.Warn, t.error?.let { "Error · $it" } ?: "Error · the engine stopped this release")
    }
    if (t.state == TorrentState.paused) {
        if (done(t)) {
            val from = t.sourceProvider?.let { "$it releases never seed" } ?: "its tracker does not seed"
            return Status(Tone.Info, "Paused after download · $from")
        }
        return Status(Tone.Warn, "Paused · $pct")
    }
    if (done(t)) {
        val who = if (t.peers == 0) "nobody downloading now" else "${plural(t.peers, "peer")} downloading"
        return Status(Tone.Ok, "Seeding · $who · ${formatSpeed(t.uploadSpeedBps)} up")
    }
    if (t.state == TorrentState.initializing) return Status(Tone.Busy, "Checking files · $pct")
    if (groupOf(t) == ReleaseGroup.Attention) return Status(Tone.Warn, "Stalled · no peers · $pct")
    val parts = mutableListOf("Downloading · $pct", formatSpeed(t.downloadSpeedBps), plural(t.peers, "peer"))
    etaSeconds(t)?.let { parts += "about ${duration(it)}" }
    return Status(Tone.Busy, parts.joinToString(" · "))
}

/** What a release's delete removes, named. */
fun deleteDescription(t: TorrentView): String {
    val names = t.files.map { it.path.substringAfterLast('/') }
    val shown = names.take(3).joinToString(", ")
    val more = if (names.size > 3) " and ${names.size - 3} more" else ""
    val files = if (names.isNotEmpty()) "${plural(names.size, "file")} ($shown$more)" else "its files"
    return "This removes $files from the server for everyone, ${formatSize(t.totalSizeBytes)} in all. " +
        "Seeding stops. It cannot be undone."
}

/** The reason beside a delete (or pause) this person cannot do. */
fun noDeleteReason(t: TorrentView): String = "Only an admin or ${t.addedByName} can do this"

/** Pause applies to a release in the swarm, Resume to a paused one. */
fun canPause(t: TorrentView): Boolean = t.state == TorrentState.live || t.state == TorrentState.initializing

fun canResume(t: TorrentView): Boolean = t.state == TorrentState.paused || t.state == TorrentState.error

/** A file's watch state for the caller: [pct] null = never started. */
@Immutable
data class WatchState(val pct: Double?, val done: Boolean) {
    val resumable: Boolean get() = pct != null && pct > 0 && !done
}

fun watchState(w: ContinueWatchingItem?): WatchState {
    if (w == null) return WatchState(null, false)
    if (w.completed) return WatchState(100.0, true)
    val d = w.durationSeconds ?: 0.0
    return WatchState(if (d > 0) min(100.0, w.positionSeconds / d * 100) else null, false)
}

fun releaseTitle(t: TorrentView, c: CollectionListItem?): String =
    c?.displayTitle ?: t.name?.let(::prettySceneName) ?: t.infohash

/** `Added by Leonard · 2d ago · from torr9 · 12 GB sent · ratio 1.20`. */
fun releaseFacts(t: TorrentView, now: java.time.Instant = java.time.Instant.now()): String {
    val ratio = ratioOf(t.uploadedBytesTotal, t.downloadedBytesTotal)
    return listOfNotNull(
        "Added by ${t.addedByName}",
        recentTime(t.addedAt, now),
        t.sourceProvider?.let { "from $it" },
        "${formatSize(t.uploadedBytesTotal)} sent",
        ratio?.let { "ratio %.2f".format(java.util.Locale.ROOT, it) },
    ).joinToString(" · ")
}

/** `42% overall · about 12 min`, or `Nothing downloading`. */
fun downloadingLine(s: HomeSummary): String {
    if (s.downloading == 0) return "Nothing downloading"
    val eta = s.downloadingEtaSeconds
    return if (eta != null) "${percent(s.downloadingPct)} overall · about ${duration(eta.toDouble())}" else "${percent(s.downloadingPct)} overall"
}

/** The library's facts line: `64 titles · 412 GB free of 2 TB · 2 downloading · 5 seeding`. */
fun libraryFacts(counts: TitleCounts?, summary: HomeSummary?): String? {
    val parts = buildList {
        counts?.let { add(plural(it.total, "title")) }
        summary?.disk?.let { add("${formatSize(it.freeBytes)} free of ${formatSize(it.totalBytes)}") }
        summary?.let {
            if (it.downloading > 0) add("${it.downloading} downloading")
            if (it.seeding > 0) add("${it.seeding} seeding")
        }
    }
    return parts.takeIf { it.isNotEmpty() }?.joinToString(" · ")
}

/** A video's state line in a file list: `1.4 GB · Watched`, `1.4 GB · 42% watched`. */
fun fileFacts(sizeBytes: Long, w: WatchState): String = when {
    w.done -> "${formatSize(sizeBytes)} · Watched"
    w.pct != null -> "${formatSize(sizeBytes)} · ${percent(w.pct)} watched"
    else -> formatSize(sizeBytes)
}
