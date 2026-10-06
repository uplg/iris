package studio.kahn.iris.tv.ui.screens.search

import java.time.OffsetDateTime
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.time.temporal.ChronoUnit
import java.util.Locale
import studio.kahn.iris.tv.data.ParsedQueryInfo
import studio.kahn.iris.tv.data.ProviderResultMeta
import studio.kahn.iris.tv.data.SearchResponse
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.SearchViewMode

/** What a search asks the trackers for, as web `search/params.ts` writes it. */
enum class SearchKind(val label: String, val apiKind: String?) {
    All("All", null),
    Movies("Movies", "movie"),
    Series("Series", "tv"),
}

/** [BestMatch] sends no `sort_by`: the server's ranker owns the order. The others go to the trackers. */
enum class SearchSort(val label: String, val sortBy: String?, val order: String?) {
    BestMatch("Best match", null, null),
    Seeders("Most seeders", "seeders", "desc"),
    Newest("Newest", "uploaded", "desc"),
    Smallest("Smallest", "size", "asc"),
    Name("Name", "title", "asc"),
    ;

    companion object {
        fun of(name: String?): SearchSort = entries.firstOrNull { it.name == name } ?: BestMatch
    }
}

val SearchViewMode.label: String
    get() = when (this) {
        SearchViewMode.TITLES -> "Titles"
        SearchViewMode.GRID -> "Grid"
        SearchViewMode.LIST -> "List"
    }

/** The search language tags (`SearchResult.language_tag`), as web `LANGUAGE_TAGS` says them. */
data class LanguageTag(val tag: String, val short: String, val long: String)

val LANGUAGE_TAGS = listOf(
    LanguageTag("fr", "French (VF)", "French audio (VF)"),
    LanguageTag("en", "English", "English audio"),
    LanguageTag("multi", "Several (MULTI)", "Several audio languages (MULTI)"),
    LanguageTag("vost", "Original with subtitles (VOSTFR)", "Original audio, French subtitles (VOSTFR)"),
    LanguageTag("vo", "Original (VO)", "Original audio (VO)"),
)

fun languageLabel(tag: String?, long: Boolean = true): String? =
    LANGUAGE_TAGS.firstOrNull { it.tag == tag }?.let { if (long) it.long else it.short }

/** One choice of the audio filter; [tag] null = any language. */
data class AudioOption(val tag: String?, val label: String, val count: Int) {
    val words: String get() = "$label · $count"
}

/** "Any language" then each tag the loaded releases carry (and the chosen one, even at 0). */
fun audioOptions(rows: List<SearchResult>, selected: String?): List<AudioOption> {
    val counts = rows.mapNotNull { it.languageTag }.groupingBy { it }.eachCount()
    return listOf(AudioOption(null, "Any language", rows.size)) +
        LANGUAGE_TAGS.filter { it.tag in counts || it.tag == selected }
            .map { AudioOption(it.tag, it.short, counts[it.tag] ?: 0) }
}

/** The audio filter is page-local: it narrows what is loaded, it is not asked of the trackers. */
fun filterLanguage(rows: List<SearchResult>, tag: String?): List<SearchResult> =
    if (tag == null) rows else rows.filter { it.languageTag == tag }

const val SEARCH_PAGE_SIZE = 25
const val MIN_QUERY = 2

/** The next page to ask after [loaded] pages, or null when the trackers have no more. */
fun nextPage(last: SearchResponse, loaded: Int): Int? {
    val pages = last.providers.maxOfOrNull { it.totalPages ?: 0 } ?: 0
    if (pages > 0) return if (loaded < pages) loaded + 1 else null
    return if (last.results.size >= SEARCH_PAGE_SIZE) loaded + 1 else null
}

/** How many pages the trackers announce, when one does. */
fun announcedPages(meta: List<ProviderResultMeta>): Int? = meta.maxOfOrNull { it.totalPages ?: 0 }?.takeIf { it > 0 }

/**
 * A tracker's page boundary can move between two asks (new uploads): page 2
 * may repeat the end of page 1. A release is shown once, keyed like the UI
 * keys it (a duplicate key crashes a lazy list).
 */
fun mergeResults(existing: List<SearchResult>, incoming: List<SearchResult>): List<SearchResult> {
    val seen = existing.mapTo(HashSet()) { releaseKey(it) }
    return existing + incoming.filter { seen.add(releaseKey(it)) }
}

/** How many of the loaded releases are each TMDB title (their `title_match`). */
fun releasesByTitle(rows: List<SearchResult>): Map<Long, Int> =
    rows.mapNotNull { it.titleMatch?.tmdbId }.groupingBy { it }.eachCount()

fun failedTrackers(meta: List<ProviderResultMeta>): List<ProviderResultMeta> = meta.filter { it.error != null }

/** "1 match in your library · 24 releases from 3 trackers · c411 did not answer" */
fun summary(matches: Int, releases: Int, meta: List<ProviderResultMeta>): String {
    val answered = meta.count { it.error == null }
    val parts = mutableListOf<String>()
    if (matches > 0) parts += "$matches ${if (matches == 1) "match" else "matches"} in your library"
    val rel = plural(releases, "release")
    parts += if (answered > 0) "$rel from ${plural(answered, "tracker")}" else rel
    val failed = failedTrackers(meta).map { it.id }
    if (failed.isNotEmpty()) parts += "${failed.joinToString(", ")} did not answer"
    return parts.joinToString(" · ")
}

/** "Page 1 of 3", or "Page 2" when no tracker says how many. */
fun pageWords(loaded: Int, meta: List<ProviderResultMeta>): String =
    announcedPages(meta)?.let { "Page $loaded of $it" } ?: "Page $loaded"

/** "Showing results for Severance · Season 2, episode 4 · 2022." */
fun parsedWords(parsed: ParsedQueryInfo?): String? {
    parsed ?: return null
    val part = parsed.season?.let { s ->
        if (parsed.episode != null && parsed.episode != 0) "Season $s, episode ${parsed.episode}" else "Season $s"
    }
    return "Showing results for " + listOfNotNull(parsed.title, part, parsed.year?.toString()).joinToString(" · ") + "."
}

private val CLOCK = DateTimeFormatter.ofPattern("HH:mm", Locale.ENGLISH)
private val DAY_MONTH = DateTimeFormatter.ofPattern("d MMM", Locale.ENGLISH)

/** When a recent search was made: "Today, 20:41", "Yesterday", "Saturday", "28 Sep". */
fun recentWhen(at: OffsetDateTime, now: ZonedDateTime): String {
    val local = at.atZoneSameInstant(now.zone)
    val days = ChronoUnit.DAYS.between(local.toLocalDate(), now.toLocalDate())
    return when {
        days <= 0L -> "Today, ${local.format(CLOCK)}"
        days == 1L -> "Yesterday"
        days < 7L -> local.dayOfWeek.getDisplayName(TextStyle.FULL, Locale.ENGLISH)
        else -> local.format(DAY_MONTH)
    }
}
