package studio.kahn.iris.tv.ui.screens.settings

import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.util.UUID
import studio.kahn.iris.tv.ui.format.OFF
import studio.kahn.iris.tv.ui.format.audioChoiceWords
import studio.kahn.iris.tv.ui.format.subtitleChoiceWords
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.DeviceView
import studio.kahn.iris.tv.data.PreferencesResponse
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError

class SettingsWordsTest {
    private val paris = ZoneId.of("Europe/Paris")
    private val now = ZonedDateTime.of(2026, 10, 6, 14, 0, 0, 0, paris)

    @Test
    fun aDeviceSaysWhenItWasPairedAndUntilWhen() {
        val paired = OffsetDateTime.parse("2026-10-06T09:05:00+02:00")
        val until = OffsetDateTime.parse("2026-11-05T09:05:00+01:00")
        val device = DeviceView(until, paired, UUID.randomUUID(), "android-tv", null)
        assertEquals("Paired today at 09:05 · Signed in until 5 Nov", device.facts(now))
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
        assertEquals(OFF, languageChoice("off"))
        assertNull(languageChoice(null))
        assertEquals("The file’s own", audioChoiceWords(null))
        assertEquals("Subtitles off", subtitleChoiceWords(OFF))
        assertEquals(COMMON_LANGUAGES, languageOptions("fr"))
        assertEquals(COMMON_LANGUAGES + "nl", languageOptions("nl"))
        assertEquals(COMMON_LANGUAGES, languageOptions(OFF))
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

    @Test
    fun aRefusedPasswordChangeIsSaidUnderItsField() {
        val wrong = UiError("This is not your current password.", "wrong_password", 400)
        assertEquals(DialogError(wrong.message, DialogField.First), passwordRefusal(wrong))
        val short = UiError("Use at least 8 characters.", "password_too_short", 400)
        assertEquals(DialogError(short.message, DialogField.Second), passwordRefusal(short))
        // anything else is the generic answer: a dead session is not a wrong password
        assertNull(passwordRefusal(UiError("This TV is signed out. Pair it again from Settings.", "unauthorized", 401)))
        assertNull(passwordRefusal(UiError("something else", "bad_request", 400)))
    }
}
