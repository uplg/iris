package studio.kahn.iris.tv.ui.screens.home

import androidx.compose.runtime.Immutable
import studio.kahn.iris.tv.data.CatalogCard
import studio.kahn.iris.tv.data.CollectionListItem
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.MediaMetadata
import studio.kahn.iris.tv.data.MoodTile
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.Shelf
import studio.kahn.iris.tv.data.WatchlistItem
import studio.kahn.iris.tv.data.tmdbBackdropUrl
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.formatSize

/** What a card leads to: OK does [CardModel.primary], hold OK (a long press) lists [CardModel.menu]. */
enum class CardAction(val label: String, val busyLabel: String = "$label…") {
    Play("Play"),
    GetAndPlay("Get and play", "Getting it…"),
    StartOver("Start over", "Starting over…"),
    OpenSeries("Open the series"),
    Open("Open"),
    FindReleases("Find releases"),
    MarkWatched("Mark as watched", "Marking as watched…"),
    RemoveFromContinue("Remove from Continue watching", "Removing…"),
    RemoveFromWatchlist("Remove from your watchlist", "Removing…"),
    NotInterested("Not interested", "Hiding it…"),
}

/**
 * One card of a home or discover row, already in words. [key] is unique on the whole
 * screen (prefixed by its row), so focus can be found again after a reload.
 */
@Immutable
data class CardModel(
    val key: String,
    val title: String,
    val art: String?,
    val kind: String? = null,
    val meta: String? = null,
    val status: String? = null,
    val tone: StatusTone = StatusTone.Muted,
    val progress: Float? = null,
    val primary: CardAction = CardAction.Open,
    val menu: List<CardAction> = emptyList(),
)

@Immutable
data class ShelfModel(val key: String, val title: String, val cards: List<CardModel>)

enum class HeroAction { Resume, Play, GetAndPlay, AllEpisodes, StartOver, Open, FindReleases }

/** One action of the hero; [busyKey] matches [HomeUiState.busy] while it travels. */
@Immutable
data class HeroButton(val action: HeroAction, val label: String, val busyLabel: String? = null, val busyKey: String? = null)

/** The home's first block: what to watch now. */
@Immutable
data class HeroModel(
    val key: String,
    val eyebrow: String,
    val title: String,
    val meta: String?,
    val overview: String?,
    val art: String?,
    val actions: List<HeroButton>,
    val languages: String? = null,
)

@Immutable
data class MoodModel(val id: String, val label: String, val art: String?, val now: String?)

/** A Continue Watching tile's identity: tiles not on disk share an empty infohash, their series tells them apart. */
fun tileKey(item: ContinueWatchingItem): String =
    item.collectionId?.let { "c:$it" } ?: "${item.infohash}:${item.fileIdx}"

const val CW_PREFIX = "cw:"
const val WATCHLIST_PREFIX = "wl:"
const val LIBRARY_PREFIX = "lib:"

private fun joined(parts: List<String?>): String? = parts.filterNotNull().filter { it.isNotBlank() }.joinToString(" · ").ifEmpty { null }

/** The title to show: TMDB's once the server trusts the match, else the release name tidied. */
fun continueTitle(item: ContinueWatchingItem, md: MediaMetadata?): String = md?.title ?: prettySceneName(item.torrentName)

fun continueCard(item: ContinueWatchingItem, md: MediaMetadata?): CardModel {
    val left = secondsLeft(item)
    val share = watchedShare(item)
    val code = episodeCode(item.season, item.episode)
    val (status, tone) = when {
        item.grabbable -> "Up next · ${nextName(item)} · Not downloaded" to StatusTone.Muted
        item.nextUp -> "Up next" to StatusTone.Muted
        else -> null to StatusTone.Muted
    }
    val menu = buildList {
        if (item.grabbable) add(CardAction.GetAndPlay) else add(CardAction.Play)
        if (isResuming(item)) add(CardAction.StartOver)
        if (item.collectionId != null) add(CardAction.OpenSeries)
        if (!item.grabbable) add(CardAction.MarkWatched)
        add(CardAction.RemoveFromContinue)
    }
    return CardModel(
        key = CW_PREFIX + tileKey(item),
        title = continueTitle(item, md),
        art = tmdbBackdropUrl(md?.backdropPath) ?: tmdbPosterUrl(item.posterPath),
        meta = joined(
            listOf(
                code ?: kindLabel(item.kind),
                if (!item.nextUp && !item.grabbable && left != null) timeLeft(left) else null,
            ),
        ),
        status = status,
        tone = tone,
        progress = share?.takeIf { !item.nextUp && it > 0f },
        primary = if (item.grabbable) CardAction.GetAndPlay else CardAction.Play,
        menu = menu,
    )
}

fun watchlistCard(item: WatchlistItem, downloading: Double?): CardModel {
    val (status, tone) = when {
        downloading != null -> "Downloading · ${percent(downloading)}" to StatusTone.Muted
        item.newCount > 0 -> plural(item.newCount, "new episode") to StatusTone.Ok
        else -> "No new episodes" to StatusTone.Muted
    }
    return CardModel(
        key = WATCHLIST_PREFIX + item.id,
        title = item.name,
        art = tmdbPosterUrl(item.posterPath),
        kind = "Series",
        meta = "Series",
        status = status,
        tone = tone,
        primary = CardAction.Open,
        menu = listOf(CardAction.Open, CardAction.RemoveFromWatchlist),
    )
}

fun libraryCard(item: CollectionListItem, downloading: Double?): CardModel {
    val kind = kindLabel(item.kind, item.isAnime == true)
    val size = if (item.kind == MediaKind.tv) plural(item.episodeCount, "episode") else formatSize(item.totalSizeBytes)
    val (status, tone) = when {
        item.ghost == true -> "No longer on disk" to StatusTone.Warn
        downloading != null -> "Downloading · ${percent(downloading)}" to StatusTone.Muted
        else -> "On disk" to StatusTone.Ok
    }
    return CardModel(
        key = LIBRARY_PREFIX + item.id,
        title = item.displayTitle,
        art = tmdbPosterUrl(item.posterPath),
        kind = kind,
        meta = "$kind · $size",
        status = status,
        tone = tone,
    )
}

/** A suggested title names a title, not a release: it leads to a search for it. */
fun catalogCard(card: CatalogCard, prefix: String): CardModel {
    val kind = kindLabel(card.kind, card.isAnime)
    val (status, tone) = when {
        card.alreadyInLibrary -> "In your library" to StatusTone.Ok
        !card.reason.isNullOrBlank() -> card.reason to StatusTone.Muted
        else -> null to StatusTone.Muted
    }
    return CardModel(
        key = prefix + card.catalogId,
        title = card.title,
        art = card.posterUrl,
        kind = kind,
        meta = joined(listOf(kind, card.year?.toString(), card.seeders?.takeIf { it > 0 }?.let { plural(it, "seeder") })),
        status = status,
        tone = tone,
        primary = CardAction.FindReleases,
        menu = listOf(CardAction.FindReleases, CardAction.NotInterested),
    )
}

/** The suggestion shelves that have something in them. */
fun shelves(list: List<Shelf>): List<ShelfModel> = list.filter { it.items.isNotEmpty() }.map { shelf ->
    val prefix = "fy:${shelf.key}:"
    ShelfModel(prefix, shelf.title, shelf.items.map { catalogCard(it, prefix) })
}

fun moodModel(tile: MoodTile): MoodModel =
    MoodModel(tile.id, tile.label, tile.backdropUrl, tile.featuredTitle?.let { "Now: $it" })

/** The first thing to resume: where it stopped, what is left, the one action that plays it from there. */
fun resumeHero(item: ContinueWatchingItem, md: MediaMetadata?, prefs: PlaybackPrefsResponse?): HeroModel {
    val key = tileKey(item)
    val left = secondsLeft(item)
    val resuming = isResuming(item)
    val length = if (resuming && left != null) timeLeft(left) else item.durationSeconds?.takeIf { it > 0 }?.let(::duration)
    val meta = if (item.kind == MediaKind.tv && item.season != null) {
        listOf(
            "Season ${item.season}",
            item.episode?.let { "Episode $it" },
            item.episodeName,
            length,
        )
    } else {
        listOf(md?.year?.toString(), kindLabel(item.kind), length)
    }
    val next = nextName(item)
    val primary = when {
        item.grabbable -> HeroButton(HeroAction.GetAndPlay, "Play $next", "Getting $next…", "get:$key")
        resuming -> HeroButton(HeroAction.Resume, "Resume at ${clock(item.positionSeconds)}")
        item.kind == MediaKind.tv -> HeroButton(HeroAction.Play, "Play $next")
        else -> HeroButton(HeroAction.Play, "Play")
    }
    val actions = buildList {
        add(primary)
        if (item.collectionId != null) add(HeroButton(HeroAction.AllEpisodes, "All episodes"))
        if (resuming) add(HeroButton(HeroAction.StartOver, "Start over", "Starting over…", "over:$key"))
    }
    return HeroModel(
        key = "resume:$key",
        eyebrow = if (item.nextUp || item.grabbable) "Up next" else "Continue where you left off",
        title = continueTitle(item, md),
        meta = joined(meta),
        overview = md?.overview?.takeIf { it.isNotBlank() },
        art = tmdbBackdropUrl(md?.backdropPath, "w1280"),
        actions = actions,
        languages = languagesLine(prefs),
    )
}

/** Nothing to resume: the freshest title of the library. */
fun libraryHero(item: CollectionListItem, md: MediaMetadata?): HeroModel = HeroModel(
    key = "library:${item.id}",
    eyebrow = "In your library",
    title = item.displayTitle,
    meta = joined(listOf(md?.year?.toString(), kindLabel(item.kind, item.isAnime == true), formatSize(item.totalSizeBytes))),
    overview = md?.overview?.takeIf { it.isNotBlank() },
    art = tmdbBackdropUrl(md?.backdropPath, "w1280"),
    actions = listOf(HeroButton(HeroAction.Open, "Open")),
)

/** The title a featured release names: the server's title match first (an indexer's own id is often wrong). */
fun featuredTitle(result: SearchResult, md: MediaMetadata?): String =
    md?.title ?: result.titleMatch?.title ?: prettySceneName(result.title)

/** A brand-new, empty library: the tracker's freshest featured release. */
fun featuredHero(result: SearchResult, md: MediaMetadata?): HeroModel {
    val match = result.titleMatch
    return HeroModel(
        key = "featured:${result.providerId}:${result.externalId}",
        eyebrow = "Featured",
        title = featuredTitle(result, md),
        meta = joined(
            listOf(
                (result.year ?: match?.year)?.toString(),
                kindLabel(result.kind ?: match?.kind),
                result.seeders?.let { plural(it.toLong(), "seeder") },
            ),
        ),
        overview = md?.overview?.takeIf { it.isNotBlank() },
        art = tmdbBackdropUrl(md?.backdropPath, "w1280"),
        actions = listOf(HeroButton(HeroAction.FindReleases, "Find releases")),
    )
}
