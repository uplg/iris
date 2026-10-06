package studio.kahn.iris.tv.ui.format

import studio.kahn.iris.tv.data.TitleWatch

// Where the person is in a title, from the server's `watch` (library cards, the search's
// library matches, a title's page), read one way everywhere (web `lib/watched.ts`).

/** Past the opening seconds: a position worth resuming (web `RESUMABLE_SECONDS`). */
const val RESUMABLE_SECONDS = 5.0

/** The one rule for "Resume" rather than "Play", everywhere: a position past [RESUMABLE_SECONDS], not finished. */
fun isResumable(positionSeconds: Double?, completed: Boolean = false): Boolean =
    !completed && positionSeconds != null && positionSeconds >= RESUMABLE_SECONDS

data class Resume(
    val infohash: String,
    val fileIdx: Int,
    /** `S2:E4`, or null for a movie. */
    val code: String?,
    /** Seconds left, when the length is known. */
    val left: Double?,
    /** Watched so far, 0 to 1, when the length is known. */
    val share: Float?,
)

/** The file to resume, when the last one watched was left mid-way. */
fun resumeOf(w: TitleWatch?): Resume? {
    if (w == null || !isResumable(w.positionSeconds, w.completed)) return null
    val length = w.durationSeconds?.takeIf { it > 0 }
    return Resume(
        infohash = w.infohash,
        fileIdx = w.fileIdx.toInt(),
        code = episodeCode(w.season, w.episode),
        left = length?.let { maxOf(0.0, it - w.positionSeconds) },
        share = length?.let { minOf(1.0, w.positionSeconds / it).toFloat() },
    )
}

/** Every episode on disk finished (a movie: its file finished). */
fun allWatched(w: TitleWatch?, series: Boolean, episodeCount: Long): Boolean = when {
    w == null -> false
    series -> episodeCount > 0 && w.watchedEpisodes >= episodeCount
    else -> w.completed
}

/** "In progress · S1:E7 · 23 min left", "Watched", "Last watched S1:E7", or null. */
fun watchWords(w: TitleWatch?, series: Boolean, episodeCount: Long): String? {
    if (w == null) return null
    if (allWatched(w, series, episodeCount)) return WATCHED
    resumeOf(w)?.let { r -> return listOfNotNull(IN_PROGRESS, r.code, r.left?.let(::timeLeft)).joinToString(" · ") }
    return episodeCode(w.season, w.episode)?.let { "Last watched $it" }
}

const val WATCHED = "Watched"
const val IN_PROGRESS = "In progress"

/** The title's watched toggle, from [allWatched]. */
fun markWatchedLabel(watched: Boolean): String = if (watched) "Mark as not watched" else "Mark as watched"

/** What the toggle did, once the server answered; [watched] is the state before it. */
fun markedWatchedWords(title: String, watched: Boolean): String =
    if (watched) "$title is marked as not watched." else "$title is marked as watched."
