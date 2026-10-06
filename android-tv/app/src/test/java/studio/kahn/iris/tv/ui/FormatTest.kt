package studio.kahn.iris.tv.ui

import org.junit.Assert.assertEquals
import org.junit.Test

class FormatTest {
    @Test
    fun sizesMatchTheWebApp() {
        assertEquals("512 B", formatSize(512))
        assertEquals("1.5 KB", formatSize(1536))
        assertEquals("12.4 GB", formatSize((12.4 * 1024 * 1024 * 1024).toLong()))
        assertEquals("120 MB", formatSize(120L * 1024 * 1024))
        assertEquals("?", formatSize(-1))
    }

    @Test
    fun speedsAreSizesPerSecond() {
        assertEquals("6.1 MB/s", formatSpeed((6.1 * 1024 * 1024).toLong()))
    }
}
