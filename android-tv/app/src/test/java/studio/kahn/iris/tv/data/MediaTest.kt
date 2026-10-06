package studio.kahn.iris.tv.data

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaTest {
    @Test
    fun recognisesVideoFilesWhateverTheCase() {
        assertTrue(isVideoPath("Show.S01E01.1080p.MKV"))
        assertTrue(isVideoPath("folder/movie.m2ts"))
        assertFalse(isVideoPath("Show.S01E01.nfo"))
        assertFalse(isVideoPath("sample.srt"))
    }
}
