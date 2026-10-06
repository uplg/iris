package studio.kahn.iris.tv.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ReleaseFilesTest {
    private fun f(index: Int, path: String, size: Long, video: Boolean = true) = ReleaseFile(index, path, size, video)

    @Test
    fun scenesMarks() {
        assertEquals(SceneMark(1, 2), sceneMark("Show.S01E02.1080p.mkv"))
        assertEquals(SceneMark(1, 2), sceneMark("dir/Show.S01.E02.mkv"))
        assertEquals(SceneMark(2, 0), sceneMark("Show.S02.MULTi"))
        assertNull(sceneMark("Movie.2006.1080p"))
    }

    @Test
    fun aPackPlaysItsFirstEpisodeAndSamplesNever() {
        val files = listOf(
            f(0, "Show/Sample/show.sample.mkv", 900),
            f(1, "Show/Show.S01E02.mkv", 100),
            f(2, "Show/Show.S01E01.mkv", 100),
            f(3, "Show/show.nfo", 1, video = false),
        )
        assertEquals(2, autoFile(files))
        assertEquals(listOf(2, 1), playableFiles(files).map { it.index })
        assertEquals("Download and play episode 1", playWords(files, 2))
        assertTrue(isSample("Show/Sample/show.sample.mkv"))
    }

    @Test
    fun aMoviePlaysItsBiggestVideo() {
        val files = listOf(f(0, "Movie/extras.mkv", 10), f(1, "Movie/movie.mkv", 1_000), f(2, "Movie/movie.srt", 5, video = false))
        assertEquals(1, autoFile(files))
        assertEquals("Download and play", playWords(files, 1))
        assertNull(autoFile(emptyList()))
        assertEquals(0, autoFile(listOf(f(0, "Movie/movie.rar", 10, video = false))))
    }
}
