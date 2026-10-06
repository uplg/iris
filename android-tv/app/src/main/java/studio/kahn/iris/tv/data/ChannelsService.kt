package studio.kahn.iris.tv.data

import android.content.ContentUris
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.core.net.toUri
import androidx.tvprovider.media.tv.Channel
import androidx.tvprovider.media.tv.PreviewProgram
import androidx.tvprovider.media.tv.TvContractCompat
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import studio.kahn.iris.tv.MainActivity

/**
 * Publishes Iris's library + continue-watching as PreviewPrograms in a
 * "Channel" on the Android TV home launcher. The launcher shows the channel
 * as a horizontal row of poster cards; clicking one opens
 * [MainActivity] with a deep-link Uri (`iris://watch/{infohash}/{fileIdx}`)
 * that bypasses Home and goes straight to playback.
 *
 * The contract:
 *  * One Channel per package (created lazily, persisted in TV provider DB)
 *  * Up to ~25 PreviewPrograms (mix of CW + library), refreshed on each
 *    [sync] call
 *  * Posters come from the user's TMDB lookup; entries without a `tmdb_id`
 *    fall back to a generic placeholder
 *
 * `sync()` is best-effort: every IO failure is swallowed so a flaky network
 * never breaks the home launcher experience. Every read happens before the
 * channel is cleared (a sync cut short never leaves it half rebuilt), the
 * TMDB reads side by side through [TmdbMetadataCache]. Cancellation
 * propagates.
 */
class ChannelsService(private val context: Context) {

    private data class Program(val title: String, val description: String?, val posterUri: String?, val deepLink: String, val type: Int)

    // The launch's sync and the one on leaving can overlap: two clear-then-insert passes
    // interleaved would leave the row doubled.
    private val syncing = Mutex()

    suspend fun sync(container: AppContainer) = syncing.withLock {
        withContext(Dispatchers.IO) {
            // Signed in only: a pairing in progress has a server and no cookies yet.
            val session = container.sessionStore.session.first()
            if (session == null || session.cookies.isEmpty()) return@withContext
            val url = session.serverUrl
            val api: IrisApi = container.apiFor(url)
            val library = async { bestEffort { api.listTorrents() }.orEmpty() }
            val cw = async { bestEffort { api.continueWatching() }.orEmpty() }
            // Pass the kind: TMDB's movie/tv id namespaces overlap, so an id-only lookup can
            // resolve to an unrelated entry and paint the wrong poster on the launcher channel.
            val watching = cw.await().take(10).map { item ->
                async {
                    val meta = item.tmdbId?.takeIf { item.tmdbVerified }
                        ?.let { TmdbMetadataCache.get(api, it, item.kind?.value) }
                    Program(
                        title = item.filePath?.substringAfterLast('/') ?: item.torrentName,
                        description = "Continue watching",
                        posterUri = tmdbPosterUrl(meta?.posterPath),
                        deepLink = "iris://watch/${item.infohash}/${item.fileIdx}",
                        type = TvContractCompat.PreviewPrograms.TYPE_MOVIE,
                    )
                }
            }
            val owned = library.await().take(15).map { t ->
                async {
                    val meta = t.tmdbId?.let { TmdbMetadataCache.get(api, it, t.kind?.value) }
                    val idx = t.files
                        .filter { f -> isVideoPath(f.path) }
                        .maxByOrNull { f -> f.sizeBytes }
                        ?.index ?: 0
                    Program(
                        title = meta?.title ?: t.name ?: t.infohash.take(12),
                        description = meta?.overview,
                        posterUri = tmdbPosterUrl(meta?.posterPath),
                        deepLink = "iris://watch/${t.infohash}/$idx",
                        type = if (meta?.kind == TmdbKind.tv) TvContractCompat.PreviewPrograms.TYPE_TV_SERIES else TvContractCompat.PreviewPrograms.TYPE_MOVIE,
                    )
                }
            }
            val programs = (watching + owned).awaitAll()
            if (programs.isEmpty()) return@withContext

            val channelId = ensureChannel()
            if (channelId < 0) return@withContext
            clearPrograms(channelId)
            var weight = programs.size + 1
            for (p in programs) {
                insertProgram(
                    channelId = channelId,
                    title = p.title,
                    description = p.description,
                    posterUri = p.posterUri,
                    deepLink = p.deepLink,
                    weight = weight--,
                    type = p.type,
                )
            }
        }
    }

    /**
     * Look up our channel by display name; create it if absent. The Channel
     * needs to be marked "browsable" by the user in Android TV settings to
     * appear on the home — `requestChannelBrowsable` opens the system prompt
     * the first time. Subsequent syncs are no-ops.
     */
    @android.annotation.SuppressLint("RestrictedApi")
    private fun ensureChannel(): Long {
        val resolver = context.contentResolver
        resolver.query(
            TvContractCompat.Channels.CONTENT_URI,
            arrayOf(
                TvContractCompat.Channels._ID,
                TvContractCompat.Channels.COLUMN_DISPLAY_NAME,
            ),
            null, null, null,
        )?.use { cursor ->
            while (cursor.moveToNext()) {
                val name = cursor.getString(1)
                if (name == "Iris") return cursor.getLong(0)
            }
        }
        val channel = Channel.Builder()
            .setType(TvContractCompat.Channels.TYPE_PREVIEW)
            .setDisplayName("Iris")
            .setAppLinkIntentUri("iris://home".toUri())
            .build()
        val uri = resolver.insert(
            TvContractCompat.Channels.CONTENT_URI,
            channel.toContentValues(),
        ) ?: return -1
        val id = ContentUris.parseId(uri)
        runCatching { TvContractCompat.requestChannelBrowsable(context, id) }
        return id
    }

    private fun clearPrograms(channelId: Long) {
        val uri = TvContractCompat.buildPreviewProgramsUriForChannel(channelId)
        runCatching { context.contentResolver.delete(uri, null, null) }
    }

    // Lint flags `PreviewProgram.Builder` setters as RestrictedApi because
    // they live in a `@RestrictTo(LIBRARY_GROUP)`-annotated class, but the
    // Android TV docs explicitly tell apps to use this builder pattern —
    // there's no public alternative. The runtime call works fine, only
    // the static analyser is wrong.
    @android.annotation.SuppressLint("RestrictedApi")
    private fun insertProgram(
        channelId: Long,
        title: String,
        description: String?,
        posterUri: String?,
        deepLink: String,
        weight: Int,
        type: Int,
    ) {
        val intent = Intent(Intent.ACTION_VIEW, deepLink.toUri()).apply {
            setPackage(context.packageName)
            setClass(context, MainActivity::class.java)
        }
        val intentUri = intent.toUri(Intent.URI_INTENT_SCHEME)
        val builder = PreviewProgram.Builder()
            .setChannelId(channelId)
            .setType(type)
            .setTitle(title)
            .setIntentUri(intentUri.toUri())
            .setWeight(weight)
        description?.let { builder.setDescription(it.take(160)) }
        posterUri?.let { builder.setPosterArtUri(it.toUri()) }
        runCatching {
            context.contentResolver.insert(
                TvContractCompat.PreviewPrograms.CONTENT_URI,
                builder.build().toContentValues(),
            )
        }
    }
}
