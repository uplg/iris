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
import kotlinx.coroutines.flow.first
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
 * never breaks the home launcher experience.
 */
class ChannelsService(private val context: Context) {

    suspend fun sync(container: AppContainer) {
        withContext(Dispatchers.IO) {
            val url = runCatching { container.sessionStore.serverUrl.first() }.getOrNull()
                ?: return@withContext
            val api: IrisApi = container.apiFor(url)
            val library = runCatching { api.listTorrents() }.getOrDefault(emptyList())
            val cw = runCatching { api.continueWatching() }.getOrDefault(emptyList())
            if (library.isEmpty() && cw.isEmpty()) return@withContext

            val channelId = ensureChannel()
            if (channelId < 0) return@withContext
            clearPrograms(channelId)

            var weight = library.size + cw.size + 1
            for (item in cw.take(10)) {
                val poster = posterUriFor(api, item.tmdbId, item.kind?.value)
                val (host, idx) = item.infohash to item.fileIdx
                insertProgram(
                    channelId = channelId,
                    title = item.filePath?.substringAfterLast('/') ?: item.torrentName,
                    description = "Continue watching",
                    posterUri = poster,
                    deepLink = "iris://watch/$host/$idx",
                    weight = weight--,
                    type = TvContractCompat.PreviewPrograms.TYPE_MOVIE,
                )
            }
            for (t in library.take(15)) {
                val meta =
                    t.tmdbId?.let { runCatching { api.tmdbMetadata(it, t.kind?.value) }.getOrNull() }
                val poster = meta?.posterPath?.let { "https://image.tmdb.org/t/p/w342$it" }
                val idx = t.files
                    .filter { f -> VIDEO_EXTS.any { f.path.endsWith(it, ignoreCase = true) } }
                    .maxByOrNull { f -> f.sizeBytes }
                    ?.index ?: 0
                insertProgram(
                    channelId = channelId,
                    title = meta?.title ?: t.name ?: t.infohash.take(12),
                    description = meta?.overview,
                    posterUri = poster,
                    deepLink = "iris://watch/${t.infohash}/$idx",
                    weight = weight--,
                    type = if (meta?.kind == TmdbKind.tv)
                        TvContractCompat.PreviewPrograms.TYPE_TV_SERIES
                    else
                        TvContractCompat.PreviewPrograms.TYPE_MOVIE,
                )
            }
        }
    }

    private suspend fun posterUriFor(api: IrisApi, tmdbId: Long?, kind: String?): String? {
        if (tmdbId == null) return null
        // Pass the kind: TMDB's movie/tv id namespaces overlap, so an
        // id-only lookup can resolve to an unrelated entry and paint the
        // wrong poster on the launcher channel.
        val meta = runCatching { api.tmdbMetadata(tmdbId, kind) }.getOrNull() ?: return null
        return meta.posterPath?.let { "https://image.tmdb.org/t/p/w342$it" }
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

    companion object {
        private val VIDEO_EXTS = listOf(
            ".mkv", ".mp4", ".webm", ".m4v", ".avi", ".mov", ".ts", ".mts", ".m2ts", ".wmv",
        )
    }
}
