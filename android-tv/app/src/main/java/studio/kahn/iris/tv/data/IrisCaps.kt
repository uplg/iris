package studio.kahn.iris.tv.data

import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.os.Build

/**
 * Build the `Iris-Caps` header for this device. See
 * `docs/SOTA_ARCHITECTURE.md` §2.2 for the wire format.
 *
 * The video-decoder list is probed from [MediaCodecList] at runtime and
 * declared per codec as `<codec>-hw` / `<codec>-sw`, so the server knows
 * exactly what this box can hardware-decode. That drives the AV1 catch-up
 * transcode: a box with no AV1 silicon (e.g. Amlogic S905X2) reports
 * `av1-sw`, which lets the server re-encode a heavy 10-bit AV1 to HEVC the
 * box decodes in hardware instead of stuttering in software.
 *
 * AV1 is special-cased: the app bundles a software AV1 decoder (the dav1d
 * `Libdav1dVideoRenderer` extension), so AV1 is *always* at least
 * software-decodable — we advertise `av1-hw` only when the device also has
 * AV1 silicon, otherwise `av1-sw`.
 */
object IrisCaps {
    /** Codec label (as the server expects) → MediaCodec MIME type. */
    private val VIDEO_MIME =
        linkedMapOf(
            "h264" to "video/avc",
            "hevc" to "video/hevc",
            "vp9" to "video/x-vnd.on2.vp9",
            "av1" to "video/av01",
        )

    /** Decoders the platform exposes, queried once. */
    private val decoders: List<MediaCodecInfo> by lazy {
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.filter { !it.isEncoder }
    }

    private fun isHardware(info: MediaCodecInfo): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            info.isHardwareAccelerated
        } else {
            // Pre-10 heuristic: Google's bundled software codecs are named
            // `OMX.google.*` / `c2.android.*`; everything else is the SoC's.
            val name = info.name.lowercase()
            !(name.startsWith("omx.google.") || name.startsWith("c2.android."))
        }

    /**
     * Whether the device has a HARDWARE decoder for `codec` (label form,
     * e.g. `"av1"`). The TV player uses this to decide whether to direct-play
     * a file or fall onto the server's transcode — it MUST agree with what
     * `headerValue()` advertises so the two stay in lockstep.
     */
    fun hasHardwareDecoder(codec: String): Boolean {
        val mime = VIDEO_MIME[codec.lowercase()] ?: return false
        return decoders.any { info ->
            info.supportedTypes.any { it.equals(mime, ignoreCase = true) } && isHardware(info)
        }
    }

    private fun videoDecoderCaps(): List<String> =
        VIDEO_MIME.mapNotNull { (label, mime) ->
            val canDecode =
                decoders.any { info -> info.supportedTypes.any { it.equals(mime, ignoreCase = true) } }
            val hw = hasHardwareDecoder(label)
            when {
                // App bundles dav1d → AV1 is always software-decodable.
                label == "av1" -> if (hw) "av1-hw" else "av1-sw"
                // No platform decoder for this codec → don't advertise it.
                !canDecode -> null
                hw -> "$label-hw"
                else -> "$label-sw"
            }
        }

    fun headerValue(): String {
        val parts =
            listOf(
                "container=mkv,mp4,webm,ts,mov,m4v",
                "vdec=${videoDecoderCaps().joinToString(",")}",
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
