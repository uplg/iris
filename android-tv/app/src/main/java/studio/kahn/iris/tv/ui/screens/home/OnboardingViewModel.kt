package studio.kahn.iris.tv.ui.screens.home

import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.GenreOption
import studio.kahn.iris.tv.data.LanguageOption
import studio.kahn.iris.tv.data.PreferencesResponse
import studio.kahn.iris.tv.data.UpdatePreferencesRequest
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.load
import studio.kahn.iris.tv.ui.state.toUiError

/** The part of the preferences a person picks (web `Picks`). */
@Immutable
data class Picks(
    val languages: List<String> = emptyList(),
    val genres: List<Long> = emptyList(),
    val includeAnime: Boolean = false,
) {
    companion object {
        fun of(p: PreferencesResponse) = Picks(p.languages, p.genres, p.includeAnime)
    }
}

/** `list` with [v] added, or taken out when it was there. */
fun <T> List<T>.toggled(v: T): List<T> = if (v in this) this - v else this + v

enum class OnboardingSave { Save, Skip }

@Immutable
data class OnboardingUiState(
    val picks: Picks = Picks(),
    val languages: Loadable<List<LanguageOption>> = Loadable.Loading,
    val genres: Loadable<List<GenreOption>> = Loadable.Loading,
    val saving: OnboardingSave? = null,
    val error: String? = null,
    /** The server kept these: the sheet closes. */
    val saved: PreferencesResponse? = null,
)

/**
 * The first-run preferences (web `Onboarding`): languages, genres and Anime, its own category.
 * "Save" and "Skip for now" both mark onboarding done on the server; the sheet closes on the
 * server's answer, a failure stays on the sheet, said.
 */
class OnboardingViewModel(private val container: AppContainer, initial: PreferencesResponse) : ViewModel() {
    private val mutable = MutableStateFlow(OnboardingUiState(picks = Picks.of(initial)))
    val state: StateFlow<OnboardingUiState> = mutable.asStateFlow()

    init {
        readOptions()
    }

    /** The languages and genres to pick from, from the server (a new one never needs an APK). */
    fun readOptions() {
        if (mutable.value.languages.valueOrNull == null) {
            mutable.update { it.copy(languages = Loadable.Loading) }
            viewModelScope.launch {
                val next = load(Loadable.Loading) { container.api().languages().languages }
                mutable.update { it.copy(languages = next) }
            }
        }
        if (mutable.value.genres.valueOrNull == null) {
            mutable.update { it.copy(genres = Loadable.Loading) }
            viewModelScope.launch {
                val next = load(Loadable.Loading) { container.api().genres().genres }
                mutable.update { it.copy(genres = next) }
            }
        }
    }

    fun toggleLanguage(value: String) = mutable.update { it.copy(picks = it.picks.copy(languages = it.picks.languages.toggled(value))) }
    fun toggleGenre(id: Long) = mutable.update { it.copy(picks = it.picks.copy(genres = it.picks.genres.toggled(id))) }
    fun toggleAnime() = mutable.update { it.copy(picks = it.picks.copy(includeAnime = !it.picks.includeAnime)) }

    fun save(keep: Boolean) {
        if (mutable.value.saving != null) return
        val picks = if (keep) mutable.value.picks else Picks()
        mutable.update { it.copy(saving = if (keep) OnboardingSave.Save else OnboardingSave.Skip, error = null) }
        viewModelScope.launch {
            try {
                val saved = container.api().savePreferences(
                    UpdatePreferencesRequest(
                        genres = picks.genres,
                        includeAnime = picks.includeAnime,
                        languages = picks.languages,
                        onboardingCompleted = true,
                    ),
                )
                mutable.update { it.copy(saving = null, saved = saved) }
            } catch (e: Exception) {
                mutable.update { it.copy(saving = null, error = e.toUiError().message) }
            }
        }
    }

    /** Put off for this visit (Back): the server is told it was skipped, the sheet closes at once. */
    fun skipInBackground() {
        container.applicationScope.launch {
            runCatching {
                container.api().savePreferences(
                    UpdatePreferencesRequest(genres = emptyList(), includeAnime = false, languages = emptyList(), onboardingCompleted = true),
                )
            }
        }
    }
}
