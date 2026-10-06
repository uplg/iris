package studio.kahn.iris.tv.ui.screens.live

import java.time.OffsetDateTime
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveCountry
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
        assertEquals(listOf("Free-to-air (TNT)", "News", "Other"), sections.map { it.title })
        assertEquals(listOf("tf1", "m6"), sections[0].channels.map { it.id })
        assertEquals(listOf("a", "c"), sections[1].channels.map { it.id })
    }

    @Test
    fun logosOnAPlateThatSuitsThem() {
        assertEquals(LogoTone.Light, toneOf(0.1))
        assertEquals(LogoTone.Dark, toneOf(0.5))
        assertEquals("a red logo: the light plate, never grey", LogoTone.Light, toneOf(0.16))
        assertEquals("a grey logo: the dark plate", LogoTone.Dark, toneOf(0.25))
        assertEquals(LogoTone.Dark, toneOf(0.9))
        assertEquals(LogoTone.Neutral, toneOf(null))
        assertEquals("https://iris/api/livetv/logo?x", absolutize("https://iris/", "/api/livetv/logo?x"))
        assertEquals("https://cdn/logo.png", absolutize("https://iris", "https://cdn/logo.png"))
    }

    private fun ch(id: String, cat: String? = null, geo: Boolean = false, dead: Boolean = false, part: Boolean = false, tnt: Int? = null) =
        LiveChannel(categories = listOfNotNull(cat), geoBlocked = geo, id = id, name = id, not247 = part, tntNumber = tnt, unreachable = dead)

    @Test
    fun categoriesByNameOtherLastAndTheUnplayableAtTheEnd() {
        val sections = channelSections(
            listOf(ch("m6", tnt = 6), ch("x"), ch("dead", "Music", dead = true), ch("tf1", tnt = 1), ch("blocked", "Music", geo = true), ch("fine", "Music"), ch("k", "Kids")),
        )
        assertEquals(listOf(TNT_SECTION, "cat:Kids", "cat:Music", "cat:Other"), sections.map { it.key })
        assertEquals(listOf("tf1", "m6"), sections[0].channels.map { it.id })
        assertEquals(listOf("fine", "dead", "blocked"), sections[2].channels.map { it.id })
    }

    @Test
    fun whyAChannelMayNotPlay() {
        assertEquals("Not answering right now", channelNotice(ch("a", geo = true, dead = true)))
        assertEquals("May be blocked in your country", channelNotice(ch("a", geo = true)))
        assertEquals("Not on air all day", channelNotice(ch("a", part = true)))
        assertNull(channelNotice(ch("a")))
    }

    private val countries = listOf(
        LiveCountry(code = "fr", flag = "🇫🇷", name = "France", channelCount = 42),
        LiveCountry(code = "gb", flag = "🇬🇧", name = "United Kingdom"),
        LiveCountry(code = "re", flag = "🇷🇪", name = "Réunion", channelCount = 1),
        LiveCountry(code = "us", flag = "🇺🇸", name = "United States", channelCount = 300),
    )

    @Test
    fun theUsualCountriesFirstAndRemembered() {
        assertEquals(listOf("us", "fr"), rememberCountry(listOf("fr"), "us"))
        assertEquals(listOf("de", "us", "fr"), rememberCountry(listOf("us", "fr", "it"), "de"))
        assertEquals(listOf("fr", "us"), usualCountries("fr", listOf("us", "fr", "zz"), countries).map { it.code })
    }

    @Test
    fun aTypedNameFindsItsCountry() {
        assertEquals(listOf("re"), findCountries(countries, "reu").map { it.code })
        assertEquals(listOf("gb", "us", "re"), findCountries(countries, "uni").map { it.code })
        assertEquals(listOf("gb"), findCountries(countries, "kingdom").map { it.code })
        assertEquals("🇫🇷 France · 42 channels", countryLabel(countries[0]))
        assertEquals("🇷🇪 Réunion · 1 channel", countryLabel(countries[2]))
        assertEquals("🇬🇧 United Kingdom", countryLabel(countries[1]))
    }
}
