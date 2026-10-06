package studio.kahn.iris.tv.ui.screens.player

import java.util.Locale
import kotlin.math.abs
import studio.kahn.iris.tv.data.EpisodePoint
import studio.kahn.iris.tv.data.EpisodeStatus
import studio.kahn.iris.tv.ui.format.clock
import studio.kahn.iris.tv.ui.format.episodeCode
import studio.kahn.iris.tv.ui.format.prettySceneName

/** Watched once past 90 % (the credits), for movies and episodes alike; the web's rule too. */
const val WATCHED_FRACTION = 0.90

/** The controls offer the next episode past 95 %. */
const val NEXT_EPISODE_FRACTION = 0.95f

fun isWatched(posMs: Long, durMs: Long?): Boolean =
    durMs != null && durMs > 0 && posMs >= durMs * WATCHED_FRACTION

fun isNearEnd(posMs: Long, durMs: Long): Boolean =
    durMs > 0 && posMs.toFloat() / durMs.toFloat() >= NEXT_EPISODE_FRACTION

/**
 * ← / → on the remote: a press moves 10 s; held, the jump grows with how long
 * the key has been down (Android's `eventTime - downTime`, so the remote's
 * repeat rate does not matter). Returns the total offset from where the press
 * started, so the preview never drifts with dropped repeats.
 */
object SeekAcceleration {
    const val STEP_MS = 10_000L
    private const val TAP_WINDOW_MS = 500L

    /** (until this hold time, media seconds gained per held second). */
    private val ramp = listOf(
        2_500L to 20L,
        6_500L to 60L,
        Long.MAX_VALUE to 180L,
    )

    fun offsetMs(heldMs: Long): Long {
        if (heldMs <= TAP_WINDOW_MS) return STEP_MS
        var offset = STEP_MS
        var from = TAP_WINDOW_MS
        for ((until, rate) in ramp) {
            val end = minOf(heldMs, until)
            if (end > from) offset += (end - from) * rate
            if (heldMs <= until) break
            from = until
        }
        return offset
    }

    /** Where a press that started at [originMs] lands after [heldMs], inside [0, durationMs]. */
    fun target(originMs: Long, forward: Boolean, heldMs: Long, durationMs: Long): Long {
        val offset = offsetMs(heldMs)
        val raw = if (forward) originMs + offset else originMs - offset
        val max = if (durationMs > 0) durationMs else Long.MAX_VALUE
        return raw.coerceIn(0L, max)
    }
}

/** A position the player's way: `32:10`, `1:02:03`. */
fun clockText(ms: Long): String = clock(ms.coerceAtLeast(0) / 1_000.0)

/** The time left, with a real minus sign: `−22:50`. */
fun remainingText(positionMs: Long, durationMs: Long): String =
    "−" + clockText(abs(durationMs - positionMs))

/** A release or file name as a title ([prettySceneName]: "Mercato (2025)"), its folders left out. */
fun prettyName(raw: String?): String? = raw
    ?.substringAfterLast('/')
    ?.takeIf { it.isNotBlank() }
    ?.let(::prettySceneName)
    ?.takeIf { it.isNotBlank() }

/** "S2:E4 · Woe's Hollow" (the name when the server knows it). */
fun episodeLine(point: EpisodePoint?): String? = point?.let {
    listOfNotNull(episodeCode(it.season, it.episode), it.name?.takeIf(String::isNotBlank)).joinToString(" · ")
}

/**
 * The next-episode rule, the web's: the controls offer it past 95 % (or at the
 * end) when it is on disk; when it is only available and the series is
 * followed, one "prepare it?" prompt per playback.
 */
object NextEpisodeRule {
    data class OnDisk(val infohash: String, val fileIdx: Int, val label: String)

    fun onDisk(next: EpisodePoint?): OnDisk? {
        val ih = next?.infohash ?: return null
        val idx = next.fileIdx ?: return null
        if (next.status != EpisodeStatus.downloaded) return null
        val name = next.name?.takeIf(String::isNotBlank) ?: episodeCode(next.season, next.episode)
        return OnDisk(ih, idx.toInt(), "Next: $name")
    }

    fun offersNext(next: EpisodePoint?, isMovie: Boolean, nearEnd: Boolean, ended: Boolean): Boolean =
        !isMovie && (nearEnd || ended) && onDisk(next) != null

    fun promptsPrepare(next: EpisodePoint?, followed: Boolean, nearEnd: Boolean, ended: Boolean, alreadyAsked: Boolean): Boolean =
        !alreadyAsked && followed && (nearEnd || ended) &&
            next?.status == EpisodeStatus.available && next.followId != null
}

/**
 * What to search when another release is wanted: the series and the episode,
 * else the title, else the cleaned-up release name.
 */
fun retrySearchQuery(title: String?, season: Long?, episode: Long?, releaseName: String?): String {
    if (!title.isNullOrBlank() && season != null && episode != null) {
        return String.format(Locale.ROOT, "%s S%02dE%02d", title, season, episode)
    }
    if (!title.isNullOrBlank()) return title
    return prettyName(releaseName)?.replace("(", "")?.replace(")", "").orEmpty()
}
