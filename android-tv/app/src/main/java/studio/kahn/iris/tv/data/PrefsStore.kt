package studio.kahn.iris.tv.data

import android.content.Context
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

/**
 * How the search results are laid out. [GRID] is the poster wall;
 * [LIST] is a dense row layout that shows the full release title at a
 * glance. Persisted so the user picks it once instead of every visit.
 */
enum class SearchViewMode { GRID, LIST }

private val Context.prefsDataStore by preferencesDataStore("iris_prefs")

private val KEY_SEARCH_VIEW_MODE = stringPreferencesKey("search_view_mode")

/**
 * Small client-side UI preferences — NOT session / auth state (that's
 * [SessionStore]). Its own DataStore file so logging out / clearing the
 * session never resets display preferences.
 */
class PrefsStore(private val context: Context) {
    val searchViewMode: Flow<SearchViewMode> = context.prefsDataStore.data
        .map { prefs: Preferences ->
            // Unknown / missing → GRID (the original behaviour), so a
            // future enum value written by a newer build degrades
            // gracefully on an older APK instead of crashing.
            when (prefs[KEY_SEARCH_VIEW_MODE]) {
                SearchViewMode.LIST.name -> SearchViewMode.LIST
                else -> SearchViewMode.GRID
            }
        }

    suspend fun setSearchViewMode(mode: SearchViewMode) {
        context.prefsDataStore.edit { it[KEY_SEARCH_VIEW_MODE] = mode.name }
    }
}
