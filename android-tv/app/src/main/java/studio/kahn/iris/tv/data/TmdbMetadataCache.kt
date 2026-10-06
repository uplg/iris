package studio.kahn.iris.tv.data

import java.util.concurrent.ConcurrentHashMap

/**
 * TMDB metadata (overview, year, genres, runtime, poster) read once per title and kind for an
 * hour, process-wide (web: one keyed query, 1 h): the library page, a release's page and the
 * Home rows share it. A failed read is not kept, so the next ask tries again; cancellation
 * propagates.
 */
object TmdbMetadataCache {
    private const val TTL_MS = 60 * 60_000L

    private data class Kept(val value: MediaMetadata, val at: Long)

    private val kept = ConcurrentHashMap<Pair<Long, String?>, Kept>()

    suspend fun get(api: IrisApi, tmdbId: Long, kind: String?, now: () -> Long = System::currentTimeMillis): MediaMetadata? {
        val key = tmdbId to kind
        kept[key]?.takeIf { now() - it.at < TTL_MS }?.let { return it.value }
        val read = bestEffort { api.tmdbMetadata(tmdbId, kind) } ?: return null
        kept[key] = Kept(read, now())
        return read
    }

    /** What is kept for [tmdbId], without a read. */
    fun peek(tmdbId: Long, kind: String?): MediaMetadata? = kept[tmdbId to kind]?.value
}
