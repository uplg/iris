package studio.kahn.iris.tv.data

import android.content.Context
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

/**
 * How the search results are laid out. [GRID] is the poster wall;
 * [LIST] is a dense row layout that shows the full release title at a
 * glance. Persisted so the user picks it once instead of every visit.
 */
enum class SearchViewMode { GRID, LIST }

private val Context.prefsDataStore by preferencesDataStore("iris_prefs")

private val KEY_SEARCH_VIEW_MODE = stringPreferencesKey("search_view_mode")

// JSON-encoded List<String>, most recent first. JSON (not a delimiter
// join) so a query containing any separator character round-trips.
private val KEY_RECENT_SEARCHES = stringPreferencesKey("recent_searches")

/** The search screen surfaces "your last N searches" — N kept tiny on
 *  purpose: a remote-friendly shortcut row, not a history browser. */
const val RECENT_SEARCHES_MAX = 3

private fun decodeRecents(raw: String?): List<String> =
    raw?.let { r -> runCatching { Json.decodeFromString<List<String>>(r) }.getOrNull() }.orEmpty()

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

    /** The device's last submitted searches, most recent first (≤
     *  [RECENT_SEARCHES_MAX]). Device-local on purpose: the TV is a
     *  living-room appliance, its recents belong to the room. */
    val recentSearches: Flow<List<String>> = context.prefsDataStore.data
        .map { prefs: Preferences -> decodeRecents(prefs[KEY_RECENT_SEARCHES]) }

    /** Record a submitted query: case-insensitively deduped to the
     *  front, capped at [RECENT_SEARCHES_MAX]. No-op for junk (< 2 chars). */
    suspend fun addRecentSearch(query: String) {
        val q = query.trim()
        if (q.length < 2) return
        context.prefsDataStore.edit { prefs ->
            val current = decodeRecents(prefs[KEY_RECENT_SEARCHES])
            val next = (listOf(q) + current.filterNot { it.equals(q, ignoreCase = true) })
                .take(RECENT_SEARCHES_MAX)
            prefs[KEY_RECENT_SEARCHES] = Json.encodeToString(next)
        }
    }
}
