package studio.kahn.iris.tv.ui.format

import java.time.Instant
import java.time.OffsetDateTime
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Each of these is said one way, wherever it shows. */
class WordsInOnePlaceTest {
    @Test
    fun countsGroupTheirThousands() {
        assertEquals("1 seeder", seedersWords(1))
        assertEquals("1,204 seeders", seedersWords(1204L))
        assertNull(seedersWords(null))
        assertEquals("3 leechers", leechersWords(3))
        assertEquals("12,000 channels", plural(12_000, "channel"))
    }

    @Test
    fun ratioEtaAndSource() {
        assertEquals("ratio 1.42", ratioWords(1.4249))
        assertEquals("about 6 min", etaWords(360.0))
        assertEquals("from torr9", fromProvider("torr9"))
    }

    @Test
    fun aTimeOfDayIs24hWhateverTheDevice() {
        val paris = ZoneId.of("Europe/Paris")
        assertEquals("21:05", clockTime(OffsetDateTime.parse("2026-10-06T19:05:00Z"), paris))
        assertEquals("09:00", clockTime(Instant.parse("2026-10-06T07:00:00Z"), paris))
    }

    @Test
    fun aReleaseLanguageInASentenceAndOnItsOwn() {
        assertEquals("several languages", languageWord("multi"))
        assertEquals("Several languages", languageChipWords("multi"))
        assertEquals("French", languageChipWords("FRENCH"))
        assertEquals("Original with French subtitles", languageChipWords("vostfr"))
        assertEquals("a tag Iris does not know, as written", "ITA", languageChipWords("ita"))
        assertNull(languageChipWords("unknown"))
        assertEquals("the search tags' short words, as the web's grid", "French (VF)", languageLabel("fr", long = false))
    }
}
