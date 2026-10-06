package studio.kahn.iris.tv.ui.screens.player

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.ProgressUpdate

class ProgressSaverTest {
    private val sent = mutableListOf<ProgressUpdate>()
    private var fail = false
    private val saver = ProgressSaver(startMs = 2_700_000L) { body, failed ->
        sent += body
        if (fail) failed()
    }
    private val dur = 3_600_000L

    @Test
    fun aHeartbeatEverySevenSecondsOfPlay() {
        saver.tick(2_703_000L, dur, playing = true)
        assertTrue(sent.isEmpty())
        saver.tick(2_707_000L, dur, playing = true)
        assertEquals(listOf(2_707.0), sent.map { it.positionSeconds })
        assertEquals(true, sent.single().playing)
        assertEquals(false, sent.single().seek)
    }

    @Test
    fun aBackwardSeekSavesAtOnceWithTheSeekFlag() {
        saver.seeked()
        saver.tick(300_000L, dur, playing = true)
        assertEquals(300.0, sent.single().positionSeconds, 0.0)
        assertEquals(true, sent.single().seek)
        // The next heartbeat counts from there, not from 45:00.
        saver.tick(307_000L, dur, playing = true)
        assertEquals(listOf(true, false), sent.map { it.seek })
    }

    @Test
    fun backToTheStartIsSavedEvenAtZero() {
        saver.seeked()
        saver.save(0L, dur, playing = false)
        assertEquals(0.0, sent.single().positionSeconds, 0.0)
        assertEquals(true, sent.single().seek)
        assertEquals(false, sent.single().playing)
    }

    @Test
    fun aPlayerThatNeverStartedSavesNothing() {
        saver.save(0L, dur, playing = false)
        assertTrue(sent.isEmpty())
    }

    @Test
    fun theSeekFlagRidesOnWhicheverSaveComesNext() {
        saver.seeked()
        saver.save(10_000L, dur, playing = true)
        assertEquals(true, sent.single().seek)
        saver.save(11_000L, dur, playing = false)
        assertEquals(false, sent.last().seek)
    }

    @Test
    fun aSeekWhilePausedIsSavedByTheTick() {
        saver.seeked()
        saver.tick(60_000L, dur, playing = false)
        assertEquals(true, sent.single().seek)
        assertEquals(false, sent.single().playing)
    }

    @Test
    fun aFailedSeekSaveHandsTheFlagOnWithoutForcingASave() {
        fail = true
        saver.seeked()
        saver.tick(20_000L, dur, playing = true)
        fail = false
        saver.tick(21_000L, dur, playing = true)
        assertEquals(1, sent.size)
        saver.tick(27_000L, dur, playing = true)
        assertEquals(listOf(true, true), sent.map { it.seek })
    }

    @Test
    fun picksAndCompletionRideAlong() {
        saver.audioIdx = 2
        saver.subtitleIdx = -1
        saver.save(3_500_000L, dur, playing = true)
        val body = sent.single()
        assertEquals(2L, body.audioTrackIdx)
        assertEquals(-1L, body.subtitleTrackIdx)
        assertTrue(body.completed == true)
        assertEquals(3_600.0, body.durationSeconds!!, 0.0)
        saver.save(3_500_000L, null, playing = true)
        assertFalse(sent.last().completed == true)
    }
}
