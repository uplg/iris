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
 * AV1 is special-cased twice:
 *   - the app bundles a software AV1 decoder (the dav1d
 *     `Libdav1dVideoRenderer` extension), so AV1 is *always* at least
 *     software-decodable;
 *   - `av1-hw` is advertised only when the silicon declares Main10
 *     ([hardwareAv1Main10]). The header is per-DEVICE so it must cover
 *     the worst case (10-bit): an 8-bit-only AV1 decoder (some TV SoCs)
 *     would runtime-fail on 10-bit streams, so that box advertises
 *     `av1-sw` and the server keeps its 10-bit catch-up transcode
 *     available for it. Per-STREAM the player is finer-grained: 8-bit
 *     AV1 happily hardware-decodes on that same box (`WatchScreen`
 *     checks the probed bit depth against [hasHardwareDecoder] /
 *     [hardwareAv1Main10] and orders the renderers accordingly).
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
     * e.g. `"av1"`). For AV1 this answers "is there silicon at all" —
     * enough for 8-bit streams; a 10-bit stream additionally needs
     * [hardwareAv1Main10].
     */
    fun hasHardwareDecoder(codec: String): Boolean {
        val mime = VIDEO_MIME[codec.lowercase()] ?: return false
        return decoders.any { info ->
            info.supportedTypes.any { it.equals(mime, ignoreCase = true) } && isHardware(info)
        }
    }

    /**
     * Whether the hardware `video/av01` decoder declares the Main10
     * profile, i.e. can be trusted with 10-bit streams. The AV1 spec makes
     * 10-bit mandatory in Main profile, but some TV SoCs ship 8-bit-only
     * decoders that omit Main10 here (or runtime-fail on 10-bit) — for
     * those, 10-bit AV1 stays on the dav1d-software / server-transcode
     * route while 8-bit still hardware-decodes. Everything keyed on this
     * MUST stay in lockstep: the `Iris-Caps` header ([headerValue]), the
     * renderer ordering (`buildPlayer`), and the proactive transcode gate
     * (`WatchScreen`).
     */
    val hardwareAv1Main10: Boolean by lazy {
        val mime = VIDEO_MIME.getValue("av1")
        decoders.any { info ->
            isHardware(info) &&
                info.supportedTypes.any { it.equals(mime, ignoreCase = true) } &&
                runCatching {
                    info.getCapabilitiesForType(mime).profileLevels.any {
                        it.profile == MediaCodecInfo.CodecProfileLevel.AV1ProfileMain10
                    }
                }.getOrDefault(false)
        }
    }

    private fun videoDecoderCaps(): List<String> =
        VIDEO_MIME.mapNotNull { (label, mime) ->
            val canDecode =
                decoders.any { info -> info.supportedTypes.any { it.equals(mime, ignoreCase = true) } }
            val hw = hasHardwareDecoder(label)
            when {
                // App bundles dav1d → AV1 is always software-decodable, and
                // only Main10-declaring silicon counts as hardware (the
                // header must cover the 10-bit worst case — see above).
                label == "av1" -> if (hardwareAv1Main10) "av1-hw" else "av1-sw"
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
