package studio.kahn.iris.tv.ui.screens.library

import androidx.compose.runtime.Immutable
import kotlin.math.max
import studio.kahn.iris.tv.data.AvailableEpisodeEntry
import studio.kahn.iris.tv.data.CollectionDetail
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.EpisodeEntry
import studio.kahn.iris.tv.data.EpisodeInfo
import studio.kahn.iris.tv.data.FileEntry
import studio.kahn.iris.tv.data.FileProgressEntry
import studio.kahn.iris.tv.data.GoneEpisodeEntry
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.SeasonPackEntry
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.isVideoPath
import studio.kahn.iris.tv.ui.format.clock
import studio.kahn.iris.tv.ui.format.duration
import studio.kahn.iris.tv.ui.format.episodeCode
import studio.kahn.iris.tv.ui.format.formatSize
import studio.kahn.iris.tv.ui.format.languageLabel
import studio.kahn.iris.tv.ui.format.percent
import studio.kahn.iris.tv.ui.format.plural
import studio.kahn.iris.tv.ui.format.recentTime
import studio.kahn.iris.tv.ui.format.timeLeft
import studio.kahn.iris.tv.ui.components.StatusTone

// A title's episodes as one row each, whatever holds them (on disk, offered, reclaimed), and
// what the page says about them: web `lib/collection/merge.ts` and `status.ts`.

@Immutable
sealed interface Variant {
    val language: String?

    data class Downloaded(override val language: String?, val infohash: String, val fileIdx: Int, val watched: Boolean) : Variant

    data class Available(
        override val language: String?,
        val indexerProvider: String,
        val quality: String?,
        val seeders: Long?,
        val sizeBytes: Long?,
    ) : Variant

    /** Reclaimed: same infohash when downloaded again, so the saved position resumes. */
    data class Gone(
        override val language: String?,
        val infohash: String,
        val fileIdx: Int,
        val watched: Boolean,
        val releaseName: String,
        val quality: String?,
        val totalSizeBytes: Long,
        val sourceProvider: String,
        val sourceExternalId: String,
    ) : Variant
}

/** One row. [absolute] set = a fleuve anime row (`Episode 1156`); season/episode stay for the grab. */
@Immutable
data class Episode(
    val season: Long,
    val episode: Long,
    val absolute: Long?,
    val variants: List<Variant>,
    val info: EpisodeInfo? = null,
) {
    val key: String get() = absolute?.let { "a$it" } ?: "$season-$episode"
}

private fun downloaded(d: EpisodeEntry) = Variant.Downloaded(d.language, d.infohash, d.fileIdx.toInt(), d.watched)

private fun gone(g: GoneEpisodeEntry) = Variant.Gone(
    language = g.language,
    infohash = g.infohash,
    fileIdx = g.fileIdx.toInt(),
    watched = g.watched,
    releaseName = g.releaseName,
    quality = g.quality,
    totalSizeBytes = g.totalSizeBytes ?: 0L,
    sourceProvider = g.sourceProvider,
    sourceExternalId = g.sourceExternalId,
)

private fun available(a: AvailableEpisodeEntry) = Variant.Available(a.language, a.indexerProvider, a.quality, a.seeders, a.sizeBytes)

/** A release on disk in the same language (or MULTI) makes "Download again" noise. */
private fun pruneShadowedGone(variants: List<Variant>): List<Variant> {
    val owned = variants.filterIsInstance<Variant.Downloaded>().map { it.language.orEmpty() }.toSet()
    if (owned.isEmpty()) return variants
    val multi = "multi" in owned
    return variants.filter { it !is Variant.Gone || (!multi && it.language.orEmpty() !in owned) }
}

private fun rank(v: Variant) = when (v) {
    is Variant.Downloaded -> 0
    is Variant.Gone -> 1
    is Variant.Available -> 2
}

private fun settle(variants: List<Variant>) =
    pruneShadowedGone(variants).sortedWith(compareBy<Variant>({ rank(it) }, { it.language.orEmpty() }))

private fun infoMap(info: List<EpisodeInfo>?) = info.orEmpty().associateBy { it.season to it.episode }

/** Seasonal layout: one row per (season, episode); episode 0 is the season-pack sentinel. */
fun mergeEpisodes(
    onDisk: List<EpisodeEntry>,
    offers: List<AvailableEpisodeEntry> = emptyList(),
    reclaimed: List<GoneEpisodeEntry> = emptyList(),
    info: List<EpisodeInfo>? = null,
): List<Episode> {
    val rows = LinkedHashMap<Pair<Long, Long>, MutableList<Variant>>()
    fun ensure(s: Long, e: Long) = rows.getOrPut(s to e) { mutableListOf() }
    onDisk.filter { it.episode != 0L }.forEach { ensure(it.season, it.episode) += downloaded(it) }
    reclaimed.filter { it.episode != 0L }.forEach { ensure(it.season, it.episode) += gone(it) }
    offers.filter { it.episode != 0L }.forEach { ensure(it.season, it.episode) += available(it) }
    val names = infoMap(info)
    return rows.map { (k, v) -> Episode(k.first, k.second, null, settle(v), names[k]) }
        .sortedWith(compareBy({ it.season }, { it.episode }))
}

/**
 * Absolute layout (One Piece): one flat list on the absolute number. Season-cut offers with no
 * absolute have no place on that axis and are left out; what is (or was) on disk always shows.
 */
fun mergeEpisodesAbsolute(
    onDisk: List<EpisodeEntry>,
    offers: List<AvailableEpisodeEntry> = emptyList(),
    reclaimed: List<GoneEpisodeEntry> = emptyList(),
    info: List<EpisodeInfo>? = null,
): List<Episode> {
    data class Row(val season: Long, val episode: Long, val absolute: Long?, val variants: MutableList<Variant> = mutableListOf())
    val rows = LinkedHashMap<String, Row>()
    fun ensure(abs: Long?, s: Long, e: Long) = rows.getOrPut(if (abs != null) "a:$abs" else "s:$s:$e") { Row(s, e, abs) }
    onDisk.filter { it.episode != 0L }.forEach { ensure(it.absoluteEpisode, it.season, it.episode).variants += downloaded(it) }
    reclaimed.filter { it.episode != 0L }.forEach { ensure(it.absoluteEpisode, it.season, it.episode).variants += gone(it) }
    for (a in offers) {
        val abs = a.absoluteEpisode ?: continue
        if (a.episode == 0L) continue
        ensure(abs, a.season, a.episode).variants += available(a)
    }
    val names = infoMap(info)
    return rows.values.map { Episode(it.season, it.episode, it.absolute, settle(it.variants), names[it.season to it.episode]) }
        .sortedWith(compareBy({ it.absolute == null }, { it.absolute ?: Long.MAX_VALUE }, { it.season }, { it.episode }))
}

@Immutable
data class Season(val season: Long, val items: List<Episode>, val packs: List<SeasonPackEntry>)

/** The seasons known, from episodes and from pack-only seasons. */
fun seasonsOf(episodes: List<Episode>, packs: List<SeasonPackEntry> = emptyList()): List<Season> {
    val seasons = (episodes.map { it.season } + packs.map { it.season }).toSortedSet()
    return seasons.map { s -> Season(s, episodes.filter { it.season == s }, packs.filter { it.season == s }) }
}

/** Season 0 is Specials: a show opens on its first real season. */
fun firstSeason(seasons: List<Season>): Long? = (seasons.firstOrNull { it.season > 0 } ?: seasons.firstOrNull())?.season

fun watchedEp(ep: Episode) = ep.variants.any {
    (it is Variant.Downloaded && it.watched) || (it is Variant.Gone && it.watched)
}

fun ownedEp(ep: Episode) = ep.variants.any { it is Variant.Downloaded }

/** `Episode 1156` on the absolute axis, else its number in its season (`Special 2`). */
fun episodeName(ep: Episode): String = when {
    ep.absolute != null -> "Episode ${ep.absolute}"
    ep.season == 0L -> "Special ${ep.episode}"
    else -> "Episode ${ep.episode}"
}

/** The row named inside an action's words: `season 2, episode 4`, `episode 1156`. */
fun episodeWords(ep: Episode): String = when {
    ep.absolute != null -> "episode ${ep.absolute}"
    ep.season == 0L -> "special ${ep.episode}"
    else -> "season ${ep.season}, episode ${ep.episode}"
}

fun seasonName(season: Long): String = if (season == 0L) "Specials" else "Season $season"

/** The row's heading: TMDB's episode name when known, beside its number. */
fun episodeTitle(ep: Episode): String {
    val name = ep.info?.name?.takeIf { it.isNotBlank() }
    return if (name != null) "${episodeName(ep)} · $name" else episodeName(ep)
}

private val WORD = mapOf(
    "french" to "French",
    "english" to "English",
    "multi" to "several languages",
    "vostfr" to "original with French subtitles",
)
// A release's language tag in the library's words, mapped to the search's tag for its label.
private val SEARCH_TAG = mapOf("french" to "fr", "english" to "en", "multi" to "multi", "vostfr" to "vost", "vo" to "vo")
private val ISO = mapOf("french" to "fr", "english" to "en")

/** A release language inside a sentence: `Play in French`. */
fun languageWord(lang: String?): String? = lang?.let { WORD[it] }

/** `English audio (original)`, `French audio (VF)`. [original] is TMDB's ISO 639-1. */
fun audioChip(lang: String, original: String? = null): String? {
    val label = languageLabel(SEARCH_TAG[lang]) ?: return null
    if (original != null && ISO[lang] == original) return "${WORD[lang]} audio (original)"
    return label
}

/** The release tags of a title as preference codes (`multi` and `unknown` name no language). */
fun releaseCodes(tags: List<String?>): List<String> = tags.mapNotNull { it?.let(ISO::get) }.distinct()

private val TOKENS = Regex("[.\\s_\\-\\[\\]()]+")

/** A movie's release language, read from its SCENE name. */
fun nameLanguage(name: String): String? {
    val tokens = name.lowercase().split(TOKENS)
    return when {
        "multi" in tokens -> "multi"
        tokens.any { it == "vostfr" || it == "subfrench" } -> "vostfr"
        tokens.any { it in setOf("french", "truefrench", "vff", "vfq", "vfi", "vf", "vf2") } -> "french"
        else -> null
    }
}

private val RES = Regex("\\b(2160p|4k|1080p|720p|576p|480p)\\b", RegexOption.IGNORE_CASE)
private val HEVC = Regex("\\b(x265|h\\.?265|hevc)\\b", RegexOption.IGNORE_CASE)
private val AV1 = Regex("\\bav1\\b", RegexOption.IGNORE_CASE)
private val H264 = Regex("\\b(x264|h\\.?264|avc)\\b", RegexOption.IGNORE_CASE)
private val HDR = Regex("\\b(hdr10\\+?|hdr|dv|dovi)\\b", RegexOption.IGNORE_CASE)

/** A release's picture in words, from its name: `1080p · HEVC · HDR`. */
fun qualityWords(name: String): String? {
    val res = RES.find(name)?.groupValues?.get(1)?.lowercase()?.let { if (it == "4k") "2160p" else it }
    val codec = when {
        HEVC.containsMatchIn(name) -> "HEVC"
        AV1.containsMatchIn(name) -> "AV1"
        H264.containsMatchIn(name) -> "H.264"
        else -> null
    }
    val hdr = if (HDR.containsMatchIn(name)) "HDR" else null
    return listOfNotNull(res, codec, hdr).takeIf { it.isNotEmpty() }?.joinToString(" · ")
}

/** The file a release plays: its biggest video. */
fun mainVideo(t: TorrentView): FileEntry? = t.files.filter { isVideoPath(it.path) }.maxByOrNull { it.sizeBytes }

@Immutable
data class PlayTarget(val infohash: String, val fileIdx: Int, val season: Long? = null, val episode: Long? = null, val absolute: Long? = null)

/** Where Play starts with no resume point: the first episode on disk, else a release's first video. */
fun firstPlayable(c: CollectionDetail): PlayTarget? {
    val owned = c.episodes.filter { it.episode > 0 }.minWithOrNull(compareBy({ it.season }, { it.episode }))
    if (owned != null) return PlayTarget(owned.infohash, owned.fileIdx.toInt(), owned.season, owned.episode, owned.absoluteEpisode)
    for (t in c.torrents) {
        val f = t.files.firstOrNull { isVideoPath(it.path) } ?: continue
        return PlayTarget(t.infohash, f.index)
    }
    return null
}

/** The resume point: the latest unfinished item of this title in Continue Watching. */
fun resumeOf(c: CollectionDetail, items: List<ContinueWatchingItem>?): ContinueWatchingItem? {
    val owned = c.torrents.map { it.infohash }.toSet()
    return items.orEmpty()
        .filter { !it.completed && !it.grabbable && it.infohash in owned }
        .maxByOrNull { it.lastWatchedAt }
}

/** The main action: `Resume S2:E4 at 32:10`, `Play S2:E5`, `Start S1:E1`, `Play`. */
fun playLabel(c: CollectionDetail, resume: ContinueWatchingItem?): String {
    if (resume != null) {
        val code = episodeCode(resume.season, resume.episode)
        if (resume.nextUp || resume.positionSeconds <= 0) return if (code != null) "Play $code" else "Play"
        val at = clock(resume.positionSeconds)
        return if (code != null) "Resume $code at $at" else "Resume at $at"
    }
    val first = firstPlayable(c)
    if (c.kind == MediaKind.tv && first?.episode != null) {
        return if (c.numbering == "absolute" && first.absolute != null) {
            "Start episode ${first.absolute}"
        } else {
            "Start ${episodeCode(first.season, first.episode)}"
        }
    }
    return "Play"
}

/** A movie with a single copy goes straight to the player: there is nothing to choose. */
fun straightToPlayer(c: CollectionDetail): PlayTarget? {
    if (c.kind != MediaKind.movie || c.torrents.size != 1) return null
    val t = c.torrents[0]
    return mainVideo(t)?.let { PlayTarget(t.infohash, it.index) }
}

/** The facts under the title: `2 seasons · 18 episodes · TMDB 8.4 · 3 releases on disk`. */
fun heroFacts(c: CollectionDetail, rows: List<Episode>, runtimeMinutes: Int?, voteScore: Double?): String {
    val out = mutableListOf<String>()
    if (c.kind == MediaKind.tv) {
        val seasons = rows.filter { it.absolute == null && it.season > 0 }.map { it.season }.distinct().size
        if (seasons > 0 && c.numbering != "absolute") out += plural(seasons, "season")
        if (rows.isNotEmpty()) out += plural(rows.size, "episode")
    } else if (runtimeMinutes != null && runtimeMinutes > 0) {
        out += duration(runtimeMinutes * 60.0)
    }
    if ((voteScore ?: 0.0) > 0) out += "TMDB %.1f".format(java.util.Locale.ROOT, (voteScore ?: 0.0) * 10)
    out += "${plural(c.torrents.size, "release")} on disk"
    return out.joinToString(" · ")
}

/** Audio and picture chips of what is on disk. */
fun heroChips(c: CollectionDetail, originalLanguage: String?): List<String> {
    val tags = if (c.kind == MediaKind.tv) c.episodes.map { it.language } else c.torrents.map { nameLanguage(it.name.orEmpty()) }
    val audio = tags.filterNotNull().distinct().mapNotNull { audioChip(it, originalLanguage) }
    val picture = c.torrents.mapNotNull { qualityWords(it.name.orEmpty()) }
    return (audio + picture).distinct()
}

/** `Downloading · 42% · done in about 6 min`, or why it is not moving. */
fun eta(t: TorrentView): String {
    if (t.state == TorrentState.paused) return "paused"
    if (t.state == TorrentState.error) return t.error?.let { "stopped: $it" } ?: "stopped by an error"
    val left = max(0L, t.totalSizeBytes - t.progressBytes)
    if (t.downloadSpeedBps <= 0) return if (t.peers > 0) "starting" else "waiting for peers"
    return "done in about ${duration(left.toDouble() / t.downloadSpeedBps)}"
}

fun downloading(t: TorrentView?): Boolean = t != null && !t.finished

/** What the row's first downloaded release does when pressed. */
enum class Verb(val words: String) {
    Resume("Resume"),
    Play("Play"),
    WatchAgain("Watch again"),
    PlayWhileDownloading("Play while downloading"),
}

@Immutable
data class RowState(val tone: StatusTone, val text: String, val progress: Float? = null, val verb: Verb? = null)

/** An episode row's state in words: what can be done with it now. */
fun rowState(
    ep: Episode,
    torrent: (String) -> TorrentView?,
    progress: (String, Int) -> FileProgressEntry?,
): RowState {
    val disk = ep.variants.filterIsInstance<Variant.Downloaded>()
    val offers = ep.variants.filterIsInstance<Variant.Available>()
    val gone = ep.variants.filterIsInstance<Variant.Gone>()
    val runtime = ep.info?.runtimeMinutes?.takeIf { it > 0 }?.let { it * 60.0 }
    if (disk.isNotEmpty()) {
        val first = disk[0]
        val t = torrent(first.infohash)
        val p = progress(first.infohash, first.fileIdx)
        val length = p?.durationSeconds?.takeIf { it > 0 } ?: runtime
        if (t != null && downloading(t)) {
            return RowState(
                if (t.state == TorrentState.error) StatusTone.Warn else StatusTone.Busy,
                "Downloading · ${percent(t.progressPct)} · ${eta(t)}",
                verb = Verb.PlayWhileDownloading,
            )
        }
        if (first.watched || p?.completed == true) {
            return RowState(StatusTone.Ok, if (length != null) "Watched · ${duration(length)}" else "Watched", verb = Verb.WatchAgain)
        }
        if (p != null && p.positionSeconds > 0) {
            val left = length?.let { " · ${timeLeft(it - p.positionSeconds)}" }.orEmpty()
            return RowState(StatusTone.Info, "In progress$left", length?.let { (p.positionSeconds / it).toFloat() }, Verb.Resume)
        }
        return RowState(StatusTone.Ok, if (length != null) "On disk · ${duration(length)}" else "On disk", verb = Verb.Play)
    }
    if (gone.isNotEmpty()) {
        val watched = gone.any { it.watched }
        return RowState(StatusTone.Info, if (watched) "Watched · removed from disk to free space" else "Removed from disk to free space")
    }
    if (offers.isNotEmpty()) {
        val langs = offers.mapNotNull { languageWord(it.language) }.distinct()
        val said = if (langs.isNotEmpty()) " · ${langs.joinToString(", ")} audio" else ""
        return RowState(StatusTone.Available, "Available · ${plural(offers.size, "release")}$said")
    }
    return RowState(StatusTone.Info, "Not available yet")
}

/** Offers grouped by language: one Grab and play per language (the server picks the best). */
fun offersByLanguage(ep: Episode): List<Variant.Available> =
    ep.variants.filterIsInstance<Variant.Available>().distinctBy { it.language.orEmpty() }

fun offerFacts(o: Variant.Available): String = listOfNotNull(
    languageWord(o.language) ?: "Unknown language",
    o.quality,
    o.seeders?.let { "$it seeders" },
    o.sizeBytes?.let(::formatSize),
).joinToString(" · ")

/** One thing an episode row can do, named with the episode and, when several, its language. */
@Immutable
sealed interface EpisodeAction {
    val label: String

    data class Play(override val label: String, val infohash: String, val fileIdx: Int) : EpisodeAction
    data class Grab(override val label: String, val season: Long, val episode: Long, val language: String?) : EpisodeAction
    data class DownloadAgain(override val label: String, val gone: Variant.Gone) : EpisodeAction
    data class Hide(override val label: String, val infohash: String) : EpisodeAction
    data class MarkWatched(override val label: String, val infohash: String, val fileIdx: Int) : EpisodeAction
}

/** The key an action waits under while the server answers. */
fun EpisodeAction.busyKey(): String = when (this) {
    is EpisodeAction.Play -> "play:$infohash:$fileIdx"
    is EpisodeAction.Grab -> "grab:$season:$episode:$language"
    is EpisodeAction.DownloadAgain -> "again:${gone.infohash}"
    is EpisodeAction.Hide -> "hide:$infohash"
    is EpisodeAction.MarkWatched -> "watched:$infohash:$fileIdx"
}

/** Every action of a row, the primary one first. */
fun episodeActions(ep: Episode, state: RowState, torrent: (String) -> TorrentView?): List<EpisodeAction> {
    val several = ep.variants.map { it.language.orEmpty() }.toSet().size > 1
    fun inLanguage(lang: String?) = if (several) languageWord(lang)?.let { " in $it" }.orEmpty() else ""
    val out = mutableListOf<EpisodeAction>()
    val disk = ep.variants.filterIsInstance<Variant.Downloaded>()
    disk.forEachIndexed { i, v ->
        val verb = when {
            i == 0 && state.verb != null -> state.verb
            downloading(torrent(v.infohash)) -> Verb.PlayWhileDownloading
            v.watched -> Verb.WatchAgain
            else -> Verb.Play
        }
        out += EpisodeAction.Play("${verb.words}${inLanguage(v.language)}", v.infohash, v.fileIdx)
    }
    offersByLanguage(ep).forEach { o ->
        out += EpisodeAction.Grab("Grab and play${inLanguage(o.language)}", ep.season, ep.episode, o.language)
    }
    ep.variants.filterIsInstance<Variant.Gone>().forEach { g ->
        out += EpisodeAction.DownloadAgain("Download again${inLanguage(g.language)}", g)
        out += EpisodeAction.Hide("Hide the removed release${inLanguage(g.language)}", g.infohash)
    }
    disk.firstOrNull { !it.watched }?.let { v ->
        if (state.verb != Verb.WatchAgain) out += EpisodeAction.MarkWatched("Mark as watched", v.infohash, v.fileIdx)
    }
    return out
}

/** `Removed · French · 1080p · 1.4 GB`. */
fun goneFacts(g: Variant.Gone): String = listOfNotNull(
    "Removed",
    languageWord(g.language),
    g.quality,
    g.totalSizeBytes.takeIf { it > 0 }?.let(::formatSize),
).joinToString(" · ")

/** A pack's facts: `French audio (VF) · 1080p · 42 seeders · 12 GB · via torr9`. */
fun packFacts(p: SeasonPackEntry): String = listOfNotNull(
    p.language?.let { audioChip(it) },
    p.quality,
    p.seeders?.let { "$it seeders" },
    p.sizeBytes?.let(::formatSize),
    "via ${p.indexerProvider}",
).joinToString(" · ")

/** The gone releases the episode list cannot show in place (a movie, a pack never split). */
fun goneReleasesBelow(c: CollectionDetail) = c.goneReleases.orEmpty().let { all ->
    val inline = c.goneEpisodes.orEmpty().filter { it.episode > 0 }.map { it.infohash }.toSet()
    all.filter { it.infohash !in inline }
}

/** A series with episodes (even a ghost) shows them; otherwise the files themselves. */
fun hasEpisodes(c: CollectionDetail): Boolean =
    c.kind == MediaKind.tv && (c.episodes.isNotEmpty() || !c.availableEpisodes.isNullOrEmpty() || !c.goneEpisodes.isNullOrEmpty())

/** The rows of a series in its own layout. */
fun episodesOf(c: CollectionDetail): List<Episode> = if (c.kind != MediaKind.tv) {
    emptyList()
} else if (c.numbering == "absolute") {
    mergeEpisodesAbsolute(c.episodes, c.availableEpisodes.orEmpty(), c.goneEpisodes.orEmpty(), c.episodeInfo)
} else {
    mergeEpisodes(c.episodes, c.availableEpisodes.orEmpty(), c.goneEpisodes.orEmpty(), c.episodeInfo)
}

/** `Season 2 · 10 episodes` or `Season 2 · watched`. */
fun seasonLabel(s: Season): String {
    val all = s.items.isNotEmpty() && s.items.all(::watchedEp)
    return "${seasonName(s.season)} · ${if (all) "watched" else plural(s.items.size, "episode")}"
}

/** `10 episodes · 7 on disk`. */
fun episodesFact(items: List<Episode>): String = "${plural(items.size, "episode")} · ${items.count(::ownedEp)} on disk"

/** Where a long list opens: a little before the first episode not watched. */
fun openingIndex(items: List<Episode>, lead: Int = 1): Int {
    val at = items.indexOfFirst { !watchedEp(it) }
    return if (at <= lead) 0 else at - lead
}

/** The watch line of a gone release: `Watched 2d ago`, `Stopped at 32:10 (42%) 3d ago`. */
fun goneWatchLine(
    watched: Boolean?,
    position: Double?,
    duration: Double?,
    lastWatched: java.time.OffsetDateTime?,
    now: java.time.Instant = java.time.Instant.now(),
): String? {
    if (watched == true) return lastWatched?.let { "Watched ${recentTime(it, now)}" } ?: "Watched"
    val pos = position ?: 0.0
    if (pos <= 0) return null
    val share = duration?.takeIf { it > 0 }?.let { percent(minOf(100.0, pos / it * 100)) }
    return listOfNotNull("Stopped at", clock(pos), share?.let { "($it)" }, lastWatched?.let { recentTime(it, now) }).joinToString(" ")
}
