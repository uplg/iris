package studio.kahn.iris.tv.data

import android.content.Context
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.hls.HlsMediaSource
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import okhttp3.OkHttpClient

/**
 * Build an `ExoPlayer` whose HTTP layer reuses our OkHttp client (cookie
 * jar, timeouts, logging). Without this Media3 spins up its own
 * `DefaultHttpDataSource`, which does NOT see our session cookies, so HLS
 * segment requests would 401 immediately.
 *
 * Optional `userAgent` lets the HTTP layer identify itself in iris-api logs
 * — useful when triaging "is this the TV or the web?".
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
        // Live offset only matters when ExoPlayer mistakes our growing-VOD
        // (EVENT-type) playlist for true live. We pin it to 0 so any
        // accidental "live" detection lands at the first segment instead
        // of "now" (= whatever segment ffmpeg has just written).
        .setLiveTargetOffsetMs(0)

    return ExoPlayer.Builder(context)
        .setMediaSourceFactory(mediaSourceFactory)
        .setSeekBackIncrementMs(10_000)
        .setSeekForwardIncrementMs(30_000)
        .build()
}

/**
 * Build a `MediaItem` for an Iris HLS stream, side-loading every text-based
 * subtitle as an external WebVTT track. Media3's player UI then surfaces
 * them in its CC menu without any extra wiring.
 */
@UnstableApi
fun buildMediaItem(
    masterUrl: String,
    subtitles: List<MediaItem.SubtitleConfiguration>,
    startPositionSeconds: Double = 0.0,
): MediaItem =
    MediaItem.Builder()
        .setUri(masterUrl)
        .setMimeType(MimeTypes.APPLICATION_M3U8)
        .setSubtitleConfigurations(subtitles)
        .setClippingConfiguration(
            MediaItem.ClippingConfiguration.Builder()
                .setStartPositionMs((startPositionSeconds * 1000).toLong().coerceAtLeast(0))
                .build()
        )
        .build()

@Suppress("unused")
private val unused = HlsMediaSource::class  // keep the explicit import to remind humans HLS module is needed
