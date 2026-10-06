package studio.kahn.iris.tv.data

import java.io.IOException
import kotlinx.coroutines.CancellationException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BestEffortTest {
    @Test
    fun aValueOrNull() {
        assertEquals(3, bestEffort { 3 })
        assertNull(bestEffort<Int> { throw IOException("offline") })
    }

    @Test(expected = CancellationException::class)
    fun cancellationPropagates() {
        bestEffort<Int> { throw CancellationException("left") }
    }
}
