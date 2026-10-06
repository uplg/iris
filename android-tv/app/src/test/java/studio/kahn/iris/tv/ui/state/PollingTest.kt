package studio.kahn.iris.tv.ui.state

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class PollingTest {
    @Test
    fun readsAtThePaceOfWhatItRead() = runTest {
        var reads = 0
        val job = launch { pollWhile({ if (reads < 2) 1_000L else 10_000L }) { reads++ } }
        runCurrent()
        assertEquals(1, reads)
        advanceTimeBy(1_001)
        assertEquals(2, reads)
        advanceTimeBy(1_001)
        assertEquals(2, reads)
        advanceTimeBy(9_000)
        assertEquals(3, reads)
        job.cancel()
    }

    @Test
    fun stopsWhenToldTo() = runTest {
        var reads = 0
        pollWhile({ 1_000L }, keepGoing = { reads < 3 }) { reads++ }
        assertEquals(3, reads)
    }

    @Test
    fun waitsForTheServerOrGivesUp() = runTest {
        var asked = 0
        assertTrue(pollUntil(2_000L, 10_000L) { ++asked == 2 })
        assertEquals(2, asked)
        asked = 0
        assertFalse(pollUntil(2_000L, 10_000L) { asked++; false })
        assertEquals(5, asked)
    }

    @Test
    fun aLiveReadIsReadAgainWhenPoked() = runTest {
        var reads = 0
        val read = LiveRead({ _: Int? -> 60_000L }) { ++reads }
        val job = launch { read.poll() }
        runCurrent()
        assertEquals(1, read.value)
        read.poke()
        runCurrent()
        assertEquals(2, read.value)
        read.refresh()
        assertEquals(3, read.value)
        job.cancel()
    }
}
