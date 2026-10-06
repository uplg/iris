package studio.kahn.iris.tv.data

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PlayerFactoryTest {
    private fun secondsAt(mbitPerS: Int) = PLAYER_BUFFER_BYTES * 8.0 / (mbitPerS * 1_000_000)

    @Test
    fun theBufferHoldsSecondsOfA4kStreamWithinA2GbBoxsMeans() {
        assertTrue("a UHD remux at its 80 Mbit/s peaks", secondsAt(80) >= 5.0)
        assertTrue("a 4K web release at 25 Mbit/s", secondsAt(25) >= 15.0)
        assertTrue("well under Media3's 130 MB default", PLAYER_BUFFER_BYTES <= 48 * 1024 * 1024)
        assertTrue(PLAYER_BACK_BUFFER_MS in 5_000..15_000)
    }

    @Test
    fun forcedTracksAreThePlayersOwn() {
        val first = ForcedTextTracks()
        val next = ForcedTextTracks()
        first.mark("3")
        assertTrue(first.isForced("3"))
        assertFalse("the next file's player starts clean", next.isForced("3"))
        assertFalse(first.isForced(null))
    }
}
