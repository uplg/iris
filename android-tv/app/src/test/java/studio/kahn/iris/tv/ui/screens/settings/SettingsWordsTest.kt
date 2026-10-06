package studio.kahn.iris.tv.ui.screens.settings

import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.util.UUID
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import studio.kahn.iris.tv.data.DeviceView
import studio.kahn.iris.tv.data.PreferencesResponse
import studio.kahn.iris.tv.ui.state.Loadable

/** Robolectric: the language codes go through Media3, which reads android.text. */
@RunWith(RobolectricTestRunner::class)
class SettingsWordsTest {
    private val paris = ZoneId.of("Europe/Paris")
    private val now = ZonedDateTime.of(2026, 10, 6, 14, 0, 0, 0, paris)

    @Test
    fun daysAreSaidAsTheWebSaysThem() {
        assertEquals("today at 09:05", onDay(now.withHour(9).withMinute(5), now))
        assertEquals("yesterday at 21:04", onDay(now.minusDays(1).withHour(21).withMinute(4), now))
        assertEquals("tomorrow at 08:00", onDay(now.plusDays(1).withHour(8), now))
        assertEquals("on Friday", onDay(now.minusDays(4), now))
        assertEquals("on 2 Sept", onDay(now.withMonth(9).withDayOfMonth(2), now))
        assertEquals("on 2 Oct 2025", onDay(now.minusYears(1).withDayOfMonth(2), now))
    }

    @Test
    fun aMomentElsewhereIsSaidInTheTvsZone() {
        val utcLateEvening = ZonedDateTime.of(2026, 10, 5, 23, 30, 0, 0, ZoneId.of("UTC"))
        assertEquals("today at 01:30", onDay(utcLateEvening, now))
    }

    @Test
    fun devicesAreNamedAsOnTheWeb() {
        val at = OffsetDateTime.parse("2026-10-01T10:00:00+02:00")
        fun device(label: String?, kind: String?) = DeviceView(at, at, UUID.randomUUID(), kind, label)
        assertEquals("Living room", device("Living room", "android-tv").name())
        assertEquals("Android TV", device("Living room", "android-tv").kindWords())
        assertEquals("Android TV", device(null, "android-tv").name())
        assertNull(device(null, "android-tv").kindWords())
        assertEquals("tizen", device("", "tizen").name())
        assertEquals("Unnamed device", device(null, null).name())
    }

    @Test
    fun playbackLanguages() {
        assertEquals("fr", languageChoice("fre"))
        assertEquals("fr", languageChoice("fr-FR"))
        assertEquals(SUBTITLES_OFF, languageChoice("off"))
        assertNull(languageChoice(null))
        assertEquals("French", languageName("fr"))
        assertEquals("Japanese", languageName("ja"))
        assertEquals("The file’s own", audioWords(null))
        assertEquals("No subtitles", subtitleWords(SUBTITLES_OFF))
        assertEquals(COMMON_LANGUAGES, languageOptions("fr"))
        assertEquals(COMMON_LANGUAGES + "nl", languageOptions("nl"))
        assertEquals(COMMON_LANGUAGES, languageOptions(SUBTITLES_OFF))
    }

    @Test
    fun theEmailSuggestsAName() {
        assertEquals("leonard", nameFromEmail("leonard.cherouvrier@kahn.studio"))
        assertEquals("", nameFromEmail(null))
    }

    @Test
    fun recommendationsKnowWhenThereIsSomethingToSave() {
        val saved = PreferencesResponse(genres = listOf(18L), includeAnime = false, languages = listOf("fr"), onboardingCompleted = true)
        val reco = Loadable.Ready(RecoOptions(saved, emptyList(), emptyList()))
        val clean = SettingsUiState(reco = reco, draft = Picks.of(saved))
        assertFalse(clean.recoDirty)
        val edited = clean.copy(draft = clean.draft!!.copy(genres = clean.draft.genres.toggled(35L)))
        assertTrue(edited.recoDirty)
        assertFalse(edited.copy(draft = edited.draft!!.copy(genres = edited.draft.genres.toggled(35L))).recoDirty)
    }
}
