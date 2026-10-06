package studio.kahn.iris.tv.ui.screens.player

import java.util.UUID
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.data.UpdatePlaybackPrefs

/**
 * The languages a scope chose itself (the web's `ownChoices` + `PlaybackChoices`). Under a title
 * ([collectionId] set) a null field is no choice of its own: it inherits the account's, so a pick
 * saves only what was chosen for that title, never a copy of the account default. Account-wide,
 * the whole state.
 */
data class LanguageChoices(
    val collectionId: UUID?,
    val audio: String? = null,
    val subtitles: String? = null,
) {
    /** An audio language picked (a track without a tag changes nothing). */
    fun audioPicked(lang: String?): LanguageChoices = if (lang == null) this else copy(audio = lang)

    /** A subtitle language picked ("off" when turned off; a track without a tag changes nothing). */
    fun subtitlePicked(lang: String?): LanguageChoices = if (lang == null) this else copy(subtitles = lang)

    fun body(): UpdatePlaybackPrefs =
        UpdatePlaybackPrefs(audioLanguage = audio, subtitleLanguage = subtitles, collectionId = collectionId)

    companion object {
        fun of(p: PlaybackPrefsResponse?, collectionId: UUID?): LanguageChoices = when {
            p == null -> LanguageChoices(collectionId)
            collectionId == null -> LanguageChoices(null, p.audioLanguage, p.subtitleLanguage)
            else -> LanguageChoices(
                collectionId,
                audio = p.audioLanguage.takeIf { p.audioForCollection == true },
                subtitles = p.subtitleLanguage.takeIf { p.subtitleForCollection == true },
            )
        }
    }
}
