package studio.kahn.iris.tv.ui.screens.player

import androidx.compose.runtime.Immutable
import java.time.Duration
import java.util.Locale
import kotlin.math.roundToInt
import studio.kahn.iris.tv.data.PlayStatus
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.ui.components.Step
import studio.kahn.iris.tv.ui.components.StepState
import studio.kahn.iris.tv.ui.format.codecWord
import studio.kahn.iris.tv.ui.format.formatSpeed
import studio.kahn.iris.tv.ui.format.percent

/** Where reading the file (`/probe`) stands. */
@Immutable
sealed interface ProbePhase {
    /** Still trying. [notOnDisk]: the server said the head is not downloaded yet. */
    data class Waiting(val notOnDisk: Boolean = true, val lastError: String? = null) : ProbePhase

    data object Done : ProbePhase

    /** Gave up (timed out, or the server refused for good). */
    data class Failed(val message: String) : ProbePhase

    /** 404: the engine no longer has this release (reclaimed for space). */
    data object Gone : ProbePhase
}

/** Something stands between the click and the picture that waiting will not fix. */
@Immutable
data class ReadyProblem(
    val title: String,
    val detail: String,
    /** Nobody shares the release: offer to remove it and pick another. */
    val deadSwarm: Boolean,
)

@Immutable
data class Readiness(val steps: List<Step>, val problem: ReadyProblem?)

/** Everything the getting-ready steps are read from. */
data class ReadyInput(
    val torrent: TorrentView?,
    val probe: ProbePhase,
    /** How it will play, once read: "1080p H.264, plays directly". */
    val playWords: String? = null,
    /** The server builds the stream first (AV1 transcode, remux fallback). */
    val serverPrep: Boolean = false,
    val serverReady: Boolean = false,
    val playStatus: PlayStatus? = null,
)

private val deadSwarmError = Regex("""no seeders|^stalled:""", RegexOption.IGNORE_CASE)

/**
 * Nobody is sharing it: the probe said so, or the snapshot shows a swarm that
 * died (no peers, no throughput, not finished, more than two minutes after
 * the add). Both timestamps are the server's, so the clock here does not
 * matter. The web's rule.
 */
fun isDeadSwarm(t: TorrentView, probeError: String?): Boolean {
    if (probeError != null && deadSwarmError.containsMatchIn(probeError)) return true
    val age = Duration.between(t.addedAt, t.fetchedAt)
    return t.state != TorrentState.initializing &&
        !t.finished &&
        t.peers == 0 &&
        t.downloadSpeedBps == 0L &&
        t.progressPct.coerceIn(0.0, 100.0) < 100.0 &&
        age > Duration.ofMinutes(2)
}


private fun peers(n: Int): String = if (n == 1) "1 peer" else "$n peers"

/**
 * The getting-ready checklist (TVPlayerStarting): each step in words with
 * its real state, the current one with how far it is, and the problem when
 * there is one. Done steps read in the past ("Connected to peers"), the
 * current one in progress ("Downloading the first minutes"), the rest as
 * what is left to do ("Start playback").
 */
fun readiness(i: ReadyInput): Readiness {
    val t = i.torrent
    val waiting = i.probe as? ProbePhase.Waiting
    val probeError = waiting?.lastError ?: (i.probe as? ProbePhase.Failed)?.message
    val problem = when {
        t != null && isDeadSwarm(t, probeError) -> ReadyProblem(
            title = "Nobody is sharing this release",
            detail = "The tracker advertised seeders, but none of them answered. Iris has " +
                "${percent(t.progressPct.coerceIn(0.0, 100.0))} of the file and no way to get the rest.",
            deadSwarm = true,
        )
        t?.state == TorrentState.error -> ReadyProblem(
            title = "The torrent stopped with an error",
            detail = t.error ?: "The engine reported a fault. Try removing it and adding it again.",
            deadSwarm = false,
        )
        i.probe is ProbePhase.Failed -> ReadyProblem("Iris could not read this file", i.probe.message, deadSwarm = false)
        i.serverPrep && i.playStatus?.error != null -> ReadyProblem(
            "The server could not prepare this file",
            i.playStatus.error,
            deadSwarm = false,
        )
        else -> null
    }

    val probed = i.probe == ProbePhase.Done
    val onDisk = probed || waiting?.notOnDisk == false || t?.finished == true
    val connected = t != null && t.state != TorrentState.initializing

    data class Raw(val done: String, val doing: String, val todo: String, val detail: String?, val progress: Float?, val met: Boolean)

    val raws = buildList {
        add(
            Raw(
                "Connected to peers", "Connecting to peers", "Connect to peers",
                detail = t?.takeIf { connected }?.let { peers(it.peers) },
                progress = null,
                met = connected,
            ),
        )
        val pct = t?.progressPct ?: 0.0
        add(
            Raw(
                "Downloaded the first minutes", "Downloading the first minutes", "Download the first minutes",
                detail = t?.let { "${percent(pct.coerceIn(0.0, 100.0))} · ${formatSpeed(it.downloadSpeedBps)}" },
                progress = (pct / 100.0).toFloat().coerceIn(0f, 1f),
                met = onDisk,
            ),
        )
        add(
            Raw(
                "Read the video details", "Reading the video details", "Read the video details",
                detail = i.playWords,
                progress = null,
                met = probed,
            ),
        )
        if (i.serverPrep) {
            val s = i.playStatus
            val fraction = s?.progress?.coerceIn(0.0, 0.99)
            val (doing, detail) = when (s?.reason) {
                "downloading" -> "Downloading on the server" to fraction?.let { percent((it * 100.0).coerceIn(0.0, 100.0)) }
                "remuxing" -> "Preparing the stream on the server" to fraction?.let { percent((it * 100.0).coerceIn(0.0, 100.0)) }
                else -> "Preparing the stream on the server" to (if (s == null) "Starting" else "Almost there")
            }
            add(
                Raw(
                    "Prepared the stream on the server", doing, "Prepare the stream on the server",
                    detail = detail,
                    progress = fraction?.toFloat(),
                    met = i.serverReady,
                ),
            )
        }
        add(Raw("Started playback", "Starting playback", "Start playback", detail = null, progress = null, met = false))
    }
    val current = raws.indexOfFirst { !it.met }
    val steps = raws.mapIndexed { k, r ->
        when {
            r.met -> Step(r.done, StepState.Done, detail = if (k == 0 || k == 2) r.detail else null)
            k == current -> Step(r.doing, StepState.Current, detail = r.detail, progress = r.progress)
            else -> Step(r.todo, StepState.Pending)
        }
    }
    return Readiness(steps, problem)
}

/** How the probed picture will play, in words: "1080p H.264, plays directly". */
fun playWords(picture: String?, route: PlayRoute): String {
    val how = when (route) {
        PlayRoute.Direct -> "plays directly"
        PlayRoute.ServerTranscode -> "converted on the server"
        PlayRoute.ServerRemux -> "remuxed on the server"
    }
    return if (picture == null) how.replaceFirstChar { it.uppercase() } else "$picture, $how"
}

/** The probed picture in words: "1080p HEVC", "2160p HEVC Dolby Vision". */
fun pictureWords(height: Int?, codec: String?, hdr: String?): String? = listOfNotNull(
    height?.let { "${it}p" },
    codec?.let { codecWord(it) ?: it.uppercase(Locale.ROOT) },
    hdr?.let(::hdrWord),
).joinToString(" ").ifEmpty { null }

/** How the bytes reach the decoder. */
enum class PlayRoute { Direct, ServerTranscode, ServerRemux }

private fun hdrWord(hdr: String): String? = when (hdr.lowercase(Locale.ROOT)) {
    "hdr10" -> "HDR10"
    "hdr10_plus" -> "HDR10+"
    "dovi" -> "Dolby Vision"
    "hlg" -> "HLG"
    else -> null
}

/**
 * The quiet facts line in the top bar: "Playing from disk · 1080p HEVC ·
 * plays directly", or "Playing while it downloads, 42%".
 */
fun factsLine(t: TorrentView?, picture: String?): String {
    val parts = mutableListOf<String>()
    if (t != null) {
        parts += if (t.finished) "Playing from disk" else "Playing while it downloads, ${percent(t.progressPct.coerceIn(0.0, 100.0))}"
    }
    if (picture != null) parts += picture
    return parts.joinToString(" · ")
}
