package studio.kahn.iris.tv.ui.screens.player

import java.util.UUID
import org.junit.Assert.assertEquals
import org.junit.Test
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.data.UpdatePlaybackPrefs
import studio.kahn.iris.tv.ui.format.OFF

class LanguageChoicesTest {
    private val avatar = UUID.randomUUID()

    @Test
    fun `a pick under a title never copies the account default into it`() {
        // The title owns nothing yet: these are the account's.
        val account = PlaybackPrefsResponse(audioLanguage = "kor", subtitleLanguage = "fre", forCollection = false)
        val off = LanguageChoices.of(account, avatar).subtitlePicked(OFF)
        assertEquals(UpdatePlaybackPrefs(audioLanguage = null, subtitleLanguage = OFF, collectionId = avatar), off.body())

        // It owns its subtitles only: the audio still inherits the account's.
        val read = PlaybackPrefsResponse(
            audioLanguage = "kor",
            subtitleLanguage = OFF,
            forCollection = true,
            audioForCollection = false,
            subtitleForCollection = true,
        )
        val subs = LanguageChoices.of(read, avatar).subtitlePicked("eng")
        assertEquals(UpdatePlaybackPrefs(audioLanguage = null, subtitleLanguage = "eng", collectionId = avatar), subs.body())
        assertEquals("fre", subs.audioPicked("fre").body().audioLanguage)
    }

    @Test
    fun `account-wide the whole state goes and a track without a tag changes nothing`() {
        val account = PlaybackPrefsResponse(audioLanguage = "kor", subtitleLanguage = "fre")
        val c = LanguageChoices.of(account, null).audioPicked(null).audioPicked("eng")
        assertEquals(UpdatePlaybackPrefs(audioLanguage = "eng", subtitleLanguage = "fre", collectionId = null), c.body())
    }

    @Test
    fun `the panel says where the choice is kept, a film being no series`() {
        assertEquals(true, keptForText(avatar, MediaKind.movie).startsWith("Kept for this film."))
        assertEquals(true, keptForText(avatar, MediaKind.tv).startsWith("Kept for the whole series."))
        assertEquals(true, keptForText(null, MediaKind.movie).startsWith("Kept as your default."))
    }
}
