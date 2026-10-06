package studio.kahn.iris.tv.ui.screens.library

import org.junit.Assert.assertEquals
import org.junit.Test

class WordsTest {
    @Test
    fun releaseAndFileNames() {
        assertEquals("The.Bear.S04.1080p.WEB.H264", releaseName("The.Bear.S04.1080p.WEB.H264.mkv"))
        assertEquals("The.Bear.S04", releaseName("The.Bear.S04"))
        assertEquals("E01.mkv", fileName("The.Bear.S04/E01.mkv"))
    }
}
