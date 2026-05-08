package studio.kahn.iris.tv.data

import android.content.Context
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

private val Context.sessionDataStore by preferencesDataStore("iris_session")

private val KEY_SERVER_URL = stringPreferencesKey("server_url")
private val KEY_SESSION_JSON = stringPreferencesKey("session_json")

/**
 * Persisted session state. The fact that we have a session is itself the
 * "logged in" signal — `cookies` is the raw httpOnly Set-Cookie payload
 * captured at /auth/login time, replayed by [SessionCookieJar].
 */
@Serializable
data class IrisSession(
    val serverUrl: String,
    val email: String,
    val isAdmin: Boolean,
    /** Raw `name=value; ...` cookie strings captured from /auth/login. */
    val cookies: List<String>,
)

class SessionStore(private val context: Context) {
    private val json = Json { ignoreUnknownKeys = true }

    val serverUrl: Flow<String?> = context.sessionDataStore.data
        .map { prefs: Preferences -> prefs[KEY_SERVER_URL] }

    val session: Flow<IrisSession?> = context.sessionDataStore.data
        .map { prefs: Preferences ->
            prefs[KEY_SESSION_JSON]?.let { runCatching { json.decodeFromString<IrisSession>(it) }.getOrNull() }
        }

    suspend fun setServerUrl(url: String) {
        context.sessionDataStore.edit { it[KEY_SERVER_URL] = url }
    }

    suspend fun saveSession(session: IrisSession) {
        context.sessionDataStore.edit {
            it[KEY_SERVER_URL] = session.serverUrl
            it[KEY_SESSION_JSON] = json.encodeToString(session)
        }
    }

    suspend fun clear() {
        context.sessionDataStore.edit { it.remove(KEY_SESSION_JSON) }
    }
}
