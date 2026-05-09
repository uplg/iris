package studio.kahn.iris.tv.data

import android.content.Context
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import okhttp3.OkHttpClient

/**
 * Build an `ExoPlayer` whose HTTP layer reuses our OkHttp client (cookie
 * jar, timeouts, logging). Without this Media3 spins up its own
 * `DefaultHttpDataSource`, which does NOT see our session cookies, so the
 * HLS manifest + segment requests would 401 immediately.
 */
@UnstableApi
fun buildPlayer(
    context: Context,
    okHttp: OkHttpClient,
    userAgent: String = "iris-tv/0.1.0 (Media3)",
): ExoPlayer {
    val dataSourceFactory = OkHttpDataSource.Factory(okHttp).setUserAgent(userAgent)
    val mediaSourceFactory = DefaultMediaSourceFactory(context)
        .setDataSourceFactory(dataSourceFactory)

    return ExoPlayer.Builder(context)
        .setMediaSourceFactory(mediaSourceFactory)
        .setSeekBackIncrementMs(10_000)
        .setSeekForwardIncrementMs(30_000)
        .build()
}

/**
 * Build a `MediaItem` for the Iris `/play/master.m3u8` endpoint — an
 * HLS-CMAF master playlist exposing one video variant + N audio
 * renditions. The `APPLICATION_M3U8` mime hint routes the
 * `DefaultMediaSourceFactory` to `HlsMediaSource`, which handles
 * multi-audio rendition switching natively via Media3's track selector.
 * Text-based subtitles are still side-loaded as external WebVTT tracks.
 */
@UnstableApi
fun buildMediaItem(
    playUrl: String,
    subtitles: List<MediaItem.SubtitleConfiguration>,
    startPositionSeconds: Double = 0.0,
): MediaItem =
    MediaItem.Builder()
        .setUri(playUrl)
        .setMimeType(MimeTypes.APPLICATION_M3U8)
        .setSubtitleConfigurations(subtitles)
        .setClippingConfiguration(
            MediaItem.ClippingConfiguration.Builder()
                .setStartPositionMs((startPositionSeconds * 1000).toLong().coerceAtLeast(0))
                .build()
        )
        .build()
