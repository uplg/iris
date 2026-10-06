package studio.kahn.iris.tv.ui.screens.player

import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.abs
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.ProgressUpdate

/** A heartbeat every 7 s of position moved (web `watch/progress.ts`). */
const val HEARTBEAT_MS = 7_000L

/**
 * Where the viewer is in a file, kept on the server, through one path (web
 * `watch/progress.ts`): the heartbeat, a pause, a track pick, Home, leaving.
 * Every save carries the current picks and the seek flag: after a deliberate
 * jump (a seek, back to the start) the next save says `seek = true`, so the
 * server's reset guard (`put_progress`: a near-zero position over 5 min of
 * stored progress is ignored without it) keeps the jump. A failed save hands
 * the flag on to the next one.
 *
 * [send] posts the body and calls its second argument when the post failed.
 * Called from the player's thread (main), except that failure callback.
 */
class ProgressSaver(
    startMs: Long,
    private val send: (ProgressUpdate, () -> Unit) -> Unit,
) {
    private var lastSavedMs = startMs
    private val seekPending = AtomicBoolean(false)
    // A jump whose save failed: it rides on the next save, without forcing one.
    private val seekUnsent = AtomicBoolean(false)

    /** Probe stream indices of the current picks; subtitle -1 = turned off. */
    @Volatile var audioIdx: Int? = null
    @Volatile var subtitleIdx: Int? = null

    /** The position jumped on purpose. */
    fun seeked() {
        seekPending.set(true)
    }

    /**
     * The 1 s tick: right after a jump (paused too), else once playback moved the position
     * [HEARTBEAT_MS] either way from the last save.
     */
    fun tick(posMs: Long, durMs: Long?, playing: Boolean) {
        when {
            seekPending.get() -> save(posMs, durMs, playing)
            playing && abs(posMs - lastSavedMs) >= HEARTBEAT_MS -> save(posMs, durMs, playing = true)
        }
    }

    /**
     * Saves [posMs] now. Nothing at 0 unless the viewer went there (a player that never
     * started must not clear a stored position).
     */
    fun save(posMs: Long, durMs: Long?, playing: Boolean) {
        val seek = seekPending.getAndSet(false) or seekUnsent.getAndSet(false)
        if (posMs <= 0 && !seek) return
        lastSavedMs = posMs
        val body = ProgressUpdate(
            positionSeconds = posMs.coerceAtLeast(0) / 1000.0,
            durationSeconds = durMs?.takeIf { it > 0 }?.div(1000.0),
            audioTrackIdx = audioIdx?.toLong(),
            subtitleTrackIdx = subtitleIdx?.toLong(),
            completed = isWatched(posMs, durMs),
            seek = seek,
            playing = playing,
        )
        send(body) { if (seek) seekUnsent.set(true) }
    }
}

/**
 * Posts what it is [send] in that order, one at a time, from a single coroutine of [scope]:
 * a slow heartbeat can't be overtaken by the save that follows it, nor land after it. [post]
 * answers whether the server took it; on false, that item's failure callback runs. [close]
 * once the last item is sent: what is queued still goes out.
 */
class SerialPoster<T>(scope: CoroutineScope, private val post: suspend (T) -> Boolean) {
    private val queue = Channel<Pair<T, () -> Unit>>(Channel.UNLIMITED)

    init {
        scope.launch {
            for ((item, failed) in queue) if (!post(item)) failed()
        }
    }

    fun send(item: T, failed: () -> Unit) {
        queue.trySend(item to failed)
    }

    fun close() {
        queue.close()
    }
}
