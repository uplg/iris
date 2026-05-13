package studio.kahn.iris.tv.data

import android.os.Build

/**
 * Build the `Iris-Caps` header for this device. See
 * `docs/SOTA_ARCHITECTURE.md` §2.2 for the wire format.
 *
 * Phase 0 declares the codecs ExoPlayer / Media3 *natively* handle on a
 * stock Android TV install. The FFmpeg-decoder extension (Phase 3) widens
 * the audio decoder list to include DTS / TrueHD; the per-device PGS
 * subtitle plugin (Phase 3) widens the subtitle list to include PGS.
 * Until those land we under-declare, which is the safe direction: the
 * server's tier-cascade falls back to the legacy HLS remux rather than
 * shipping us a blob we can't play.
 */
object IrisCaps {
    fun headerValue(): String {
        val parts = listOf(
            "container=mkv,mp4,webm,ts,mov,m4v",
            "vdec=h264,hevc,av1,vp9",
            "adec=aac,ac3,eac3,flac,mp3,opus,vorbis",
            "subs=webvtt,srt,ssa,ttml",
            "mse=0",
            "webcodecs=0",
            "webgpu=0",
            "platform=android-tv-${Build.VERSION.RELEASE}-sdk${Build.VERSION.SDK_INT}",
        )
        return parts.joinToString("; ")
    }
}

/** Header name. Lower-cased per RFC 7230 — Iris-Caps is treated case-
 *  insensitively by both Axum's header machinery and OkHttp. */
const val IRIS_CAPS_HEADER: String = "Iris-Caps"
