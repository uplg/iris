package studio.kahn.iris.tv.data

import android.content.Context
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

/**
 * How the search results are laid out (web `search/view.ts`): [TITLES] the
 * TMDB titles the words could mean (the default), [GRID] a poster per
 * release, [LIST] a row per release with its full name. Kept per device.
 */
enum class SearchViewMode { TITLES, GRID, LIST }

private val Context.prefsDataStore by preferencesDataStore("iris_prefs")

private val KEY_SEARCH_VIEW_MODE = stringPreferencesKey("search_view_mode")
private val KEY_LIBRARY_VIEW = stringPreferencesKey("library_view")

/**
 * Small client-side UI preferences — NOT session / auth state (that's
 * [SessionStore]). Its own DataStore file so logging out / clearing the
 * session never resets display preferences.
 */
class PrefsStore(private val context: Context) {
    val searchViewMode: Flow<SearchViewMode> = context.prefsDataStore.data
        .map { prefs: Preferences ->
            // Unknown or missing reads as TITLES: a value written by a newer
            // build degrades instead of crashing.
            SearchViewMode.entries.firstOrNull { it.name == prefs[KEY_SEARCH_VIEW_MODE] } ?: SearchViewMode.TITLES
        }

    suspend fun setSearchViewMode(mode: SearchViewMode) {
        context.prefsDataStore.edit { it[KEY_SEARCH_VIEW_MODE] = mode.name }
    }

    /**
     * The library's view last chosen on this device (web `LIBRARY_VIEW_KEY`), by name; null
     * until one is chosen. The library reads an unknown name as its first view.
     */
    val libraryView: Flow<String?> = context.prefsDataStore.data.map { it[KEY_LIBRARY_VIEW] }

    suspend fun setLibraryView(name: String) {
        context.prefsDataStore.edit { it[KEY_LIBRARY_VIEW] = name }
    }
}
