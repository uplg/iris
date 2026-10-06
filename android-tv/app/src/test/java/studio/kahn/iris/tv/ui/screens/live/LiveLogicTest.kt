package studio.kahn.iris.tv.ui.screens.live

import java.time.OffsetDateTime
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveProgramme

class LiveLogicTest {
    private val start = OffsetDateTime.parse("2026-10-06T20:00:00Z")
    private val stop = OffsetDateTime.parse("2026-10-06T21:00:00Z")
    private val clock = { t: OffsetDateTime -> String.format(Locale.ROOT, "%02d:%02d", t.hour, t.minute) }

    @Test
    fun howFarIntoTheProgramme() {
        val half = start.plusMinutes(30).toInstant().toEpochMilli()
        assertEquals(0.5f, programmeProgress(start, stop, half)!!, 0.001f)
        assertNull(programmeProgress(start, stop, stop.plusMinutes(1).toInstant().toEpochMilli()))
        assertNull(programmeProgress(stop, start, half))
    }

    @Test
    fun nowAndNextInWords() {
        val p = LiveProgramme(start = start, stop = stop, title = "News")
        assertEquals("Now: News, until 21:00", nowWords(p, clock))
        assertEquals("Next at 20:00: News", nextWords(p, clock))
        assertEquals("Now: News", nowWords(p) { "" })
    }

    @Test
    fun tntFirstThenByCategory() {
        fun ch(id: String, tnt: Int? = null, cat: String? = null) =
            LiveChannel(categories = listOfNotNull(cat), geoBlocked = false, id = id, name = id, not247 = false, tntNumber = tnt)
        val sections = channelSections(listOf(ch("a", cat = "News"), ch("tf1", tnt = 1), ch("b"), ch("c", cat = "News"), ch("m6", tnt = 6)))
        assertEquals(listOf("TNT", "News", "Other"), sections.map { it.title })
        assertEquals(listOf("tf1", "m6"), sections[0].channels.map { it.id })
        assertEquals(listOf("a", "c"), sections[1].channels.map { it.id })
    }

    @Test
    fun logosOnAPlateThatSuitsThem() {
        assertEquals(LogoTone.Light, toneOf(0.1))
        assertEquals(LogoTone.Neutral, toneOf(0.5))
        assertEquals(LogoTone.Dark, toneOf(0.9))
        assertEquals(LogoTone.Neutral, toneOf(null))
        assertEquals("https://iris/api/livetv/logo?x", absolutize("https://iris/", "/api/livetv/logo?x"))
        assertEquals("https://cdn/logo.png", absolutize("https://iris", "https://cdn/logo.png"))
    }
}
