package studio.kahn.iris.tv.ui.screens.settings

import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.ChangeDisplayNameRequest
import studio.kahn.iris.tv.data.ChangePasswordRequest
import studio.kahn.iris.tv.data.DeviceView
import studio.kahn.iris.tv.data.GenreOption
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.LanguageOption
import studio.kahn.iris.tv.data.LinkRequest
import studio.kahn.iris.tv.data.PasskeyView
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.data.PreferencesResponse
import studio.kahn.iris.tv.data.UpdatePlaybackPrefs
import studio.kahn.iris.tv.data.UpdatePreferencesRequest
import studio.kahn.iris.tv.data.UserResponse
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError
import studio.kahn.iris.tv.ui.state.load
import studio.kahn.iris.tv.ui.state.toUiError

/** The parts of Settings, in rail order. */
enum class SettingsSection(val label: String) {
    You("You"),
    Playback("Playback"),
    Recommendations("Recommendations"),
    Devices("Devices"),
    Passkeys("Passkeys"),
    Password("Password"),
    App("App update"),
    ThisTv("This TV"),
}

/** What "For You" is tuned by: the part a person picks. */
@Immutable
data class Picks(val languages: List<String>, val genres: List<Long>, val includeAnime: Boolean) {
    companion object {
        fun of(p: PreferencesResponse) = Picks(p.languages, p.genres, p.includeAnime)
    }
}

/** `list` with [v] added, or taken out when it was there. */
fun <T> List<T>.toggled(v: T): List<T> = if (v in this) this - v else this + v

@Immutable
data class RecoOptions(
    val saved: PreferencesResponse,
    val languages: List<LanguageOption>,
    val genres: List<GenreOption>,
)

/** The dialog or side panel open over Settings. */
@Immutable
sealed interface SettingsDialog {
    data object Rename : SettingsDialog
    data object Password : SettingsDialog
    data object Pair : SettingsDialog
    data object Audio : SettingsDialog
    data object Subtitles : SettingsDialog
    data class Revoke(val device: DeviceView) : SettingsDialog
    /** Another server means signing out here and pairing there. */
    data object ChangeServer : SettingsDialog
}

/** What the last action did, said in its section. */
@Immutable
data class Outcome(val section: SettingsSection, val text: String, val failed: Boolean = false)

/** Which field of the open dialog an error is about. */
enum class DialogField { First, Second }

@Immutable
data class DialogError(val text: String, val field: DialogField = DialogField.First)

@Immutable
data class SettingsUiState(
    val serverUrl: String? = null,
    val account: Loadable<UserResponse> = Loadable.Loading,
    val playback: Loadable<PlaybackPrefsResponse> = Loadable.Loading,
    val reco: Loadable<RecoOptions> = Loadable.Loading,
    /** The recommendation picks being edited; equal to the saved ones when nothing changed. */
    val draft: Picks? = null,
    val devices: Loadable<List<DeviceView>> = Loadable.Loading,
    val passkeys: Loadable<List<PasskeyView>> = Loadable.Loading,
    val dialog: SettingsDialog? = null,
    val dialogError: DialogError? = null,
    /** The action running ([Busy]), its control shows it. */
    val busy: String? = null,
    val outcome: Outcome? = null,
    /** A code was accepted: the list is read again until the new device appears. */
    val waitingForDevice: Boolean = false,
    val signedOut: Boolean = false,
    /** Counts the saved name changes, for the header to read the name again. */
    val nameChanges: Int = 0,
) {
    val recoDirty: Boolean
        get() = draft != null && reco.valueOrNull?.let { Picks.of(it.saved) } != draft
}

/** Keys of [SettingsUiState.busy]. */
object Busy {
    const val RENAME = "rename"
    const val USE_EMAIL = "use-email"
    const val AUDIO = "audio"
    const val SUBTITLES = "subtitles"
    const val RECO = "reco"
    const val PAIR = "pair"
    const val PASSWORD = "password"
    const val SIGN_OUT = "sign-out"
    fun revoke(device: DeviceView) = "revoke:${device.jti}"
}

const val PASSWORD_MIN = 8

/** Settings and the account (the web's /account, minus what a TV can't do). */
class SettingsViewModel(private val container: AppContainer) : ViewModel() {
    private val mutable = MutableStateFlow(SettingsUiState())
    val state: StateFlow<SettingsUiState> = mutable.asStateFlow()
    private var waiting: Job? = null

    init {
        refresh()
    }

    private suspend fun api(): IrisApi {
        val url = container.sessionStore.serverUrl.first() ?: error("No Iris server is set on this TV.")
        return container.apiFor(url)
    }

    fun refresh() {
        viewModelScope.launch {
            mutable.update { it.copy(serverUrl = container.sessionStore.serverUrl.first()) }
            launch { readAccount() }
            launch { readPlayback() }
            launch { readReco() }
            launch { readDevices() }
            launch {
                val next = load(mutable.value.passkeys) { api().listPasskeys() }
                mutable.update { it.copy(passkeys = next) }
            }
        }
    }

    private suspend fun readAccount() {
        val next = load(mutable.value.account) { api().me() }
        mutable.update { it.copy(account = next) }
    }

    private suspend fun readPlayback() {
        val next = load(mutable.value.playback) { api().playbackPreferences() }
        mutable.update { it.copy(playback = next) }
    }

    private suspend fun readDevices() {
        val next = load(mutable.value.devices) { api().listDevices() }
        mutable.update { it.copy(devices = next) }
    }

    private suspend fun readReco() {
        val next = load(mutable.value.reco) {
            val a = api()
            val (prefs, languages, genres) = coroutineScope3(
                { a.preferences() },
                { orNone { a.languages().languages } },
                { orNone { a.genres().genres } },
            )
            RecoOptions(prefs, languages, genres)
        }
        mutable.update { s ->
            val saved = next.valueOrNull?.saved
            s.copy(reco = next, draft = if (s.draft == null && saved != null) Picks.of(saved) else s.draft)
        }
    }

    fun open(dialog: SettingsDialog) = mutable.update { it.copy(dialog = dialog, dialogError = null, outcome = null) }

    fun closeDialog() = mutable.update { it.copy(dialog = null, dialogError = null) }

    /**
     * Runs one action: [busy] set meanwhile (a second press is ignored), its
     * outcome said in [section] or, with [inDialog], under the dialog's field.
     * [refused] turns a server refusal into words for a field.
     */
    private fun act(
        key: String,
        section: SettingsSection,
        inDialog: Boolean = false,
        refused: (UiError) -> DialogError? = { null },
        block: suspend (IrisApi) -> String?,
    ) {
        if (mutable.value.busy != null) return
        mutable.update { it.copy(busy = key, outcome = null, dialogError = null) }
        viewModelScope.launch {
            try {
                val said = block(api())
                mutable.update {
                    it.copy(
                        busy = null,
                        dialog = if (inDialog) null else it.dialog,
                        outcome = said?.let { text -> Outcome(section, text) },
                    )
                }
            } catch (e: Exception) {
                val error = e.toUiError()
                val field = if (inDialog) refused(error) ?: DialogError(error.message) else null
                mutable.update {
                    it.copy(
                        busy = null,
                        dialogError = field,
                        outcome = if (field == null) Outcome(section, error.message, failed = true) else null,
                    )
                }
            }
        }
    }

    fun rename(name: String) {
        val clean = name.trim()
        if (clean.isEmpty()) {
            mutable.update { it.copy(dialogError = DialogError("Enter a name.")) }
            return
        }
        act(Busy.RENAME, SettingsSection.You, inDialog = true) { api ->
            saveName(api, clean)
        }
    }

    fun useEmailName() {
        val name = nameFromEmail(mutable.value.account.valueOrNull?.email)
        if (name.isEmpty()) return
        act(Busy.USE_EMAIL, SettingsSection.You) { api -> saveName(api, name) }
    }

    /** Saved, then read back: what shows is the server's name. */
    private suspend fun saveName(api: IrisApi, name: String): String {
        api.changeDisplayName(ChangeDisplayNameRequest(displayName = name))
        val me = api.me()
        mutable.update { it.copy(account = Loadable.Ready(me), nameChanges = it.nameChanges + 1) }
        return "Display name changed to ${me.displayName}."
    }

    /** [choice] null = the file's own. Sends both languages, as the endpoint wants. */
    fun saveAudio(choice: String?) = savePlayback(Busy.AUDIO, "Audio language saved.") { it.copy(audioLanguage = choice) }

    fun saveSubtitles(choice: String?) = savePlayback(Busy.SUBTITLES, "Subtitle language saved.") { it.copy(subtitleLanguage = choice) }

    private fun savePlayback(key: String, said: String, change: (UpdatePlaybackPrefs) -> UpdatePlaybackPrefs) {
        val current = mutable.value.playback.valueOrNull ?: return
        act(key, SettingsSection.Playback, inDialog = true) { api ->
            api.savePlaybackPreferences(
                change(UpdatePlaybackPrefs(audioLanguage = current.audioLanguage, subtitleLanguage = current.subtitleLanguage)),
            )
            mutable.update { it.copy(playback = Loadable.Ready(api.playbackPreferences())) }
            said
        }
    }

    fun toggleLanguage(value: String) = editDraft { it.copy(languages = it.languages.toggled(value)) }

    fun toggleGenre(id: Long) = editDraft { it.copy(genres = it.genres.toggled(id)) }

    fun toggleAnime() = editDraft { it.copy(includeAnime = !it.includeAnime) }

    private fun editDraft(change: (Picks) -> Picks) =
        mutable.update { s -> s.draft?.let { s.copy(draft = change(it), outcome = null) } ?: s }

    fun saveReco() {
        val s = mutable.value
        val draft = s.draft ?: return
        val saved = s.reco.valueOrNull?.saved ?: return
        if (!s.recoDirty) return
        act(Busy.RECO, SettingsSection.Recommendations) { api ->
            val kept = api.savePreferences(
                UpdatePreferencesRequest(
                    genres = draft.genres,
                    includeAnime = draft.includeAnime,
                    languages = draft.languages,
                    onboardingCompleted = saved.onboardingCompleted,
                ),
            )
            mutable.update { st ->
                val options = st.reco.valueOrNull
                st.copy(
                    reco = options?.let { Loadable.Ready(it.copy(saved = kept)) } ?: st.reco,
                    draft = Picks.of(kept),
                )
            }
            "Recommendations saved."
        }
    }

    fun pair(code: String, label: String) {
        val clean = code.trim().uppercase()
        if (clean.length < 4) {
            mutable.update { it.copy(dialogError = DialogError("Enter the code the other TV shows, like WX7K-ABCD.")) }
            return
        }
        val had = mutable.value.devices.valueOrNull?.size ?: 0
        act(Busy.PAIR, SettingsSection.Devices, inDialog = true) { api ->
            api.linkDevice(LinkRequest(code = clean, label = label.trim().takeIf { it.isNotEmpty() }))
            waitForDevice(had)
            "Code accepted. Waiting for the other TV to sign in: it appears below when it does."
        }
    }

    /** Reads the list every 2 s until it grows or the code's life (10 min on the server) is over. */
    private fun waitForDevice(had: Int) {
        waiting?.cancel()
        mutable.update { it.copy(waitingForDevice = true) }
        waiting = viewModelScope.launch {
            val tries = CODE_LIFE_MS / DEVICE_POLL_MS
            repeat(tries.toInt()) {
                delay(DEVICE_POLL_MS)
                readDevices()
                if ((mutable.value.devices.valueOrNull?.size ?: 0) > had) {
                    mutable.update {
                        it.copy(waitingForDevice = false, outcome = Outcome(SettingsSection.Devices, "The other TV is paired and signed in."))
                    }
                    return@launch
                }
            }
            mutable.update {
                it.copy(
                    waitingForDevice = false,
                    outcome = Outcome(
                        SettingsSection.Devices,
                        "The other TV did not sign in in time. Show a new code on it and enter it here.",
                        failed = true,
                    ),
                )
            }
        }
    }

    fun stopWaiting() {
        waiting?.cancel()
        mutable.update { it.copy(waitingForDevice = false, outcome = null) }
    }

    fun revoke(device: DeviceView) {
        closeDialog()
        act(Busy.revoke(device), SettingsSection.Devices) { api ->
            api.revokeDevice(device.jti.toString())
            mutable.update { it.copy(devices = Loadable.Ready(api.listDevices())) }
            "${device.name()} signed out."
        }
    }

    fun changePassword(current: String, next: String) {
        when {
            current.isEmpty() -> return mutable.update { it.copy(dialogError = DialogError("Enter your current password.")) }
            next.length < PASSWORD_MIN ->
                return mutable.update { it.copy(dialogError = DialogError("Use at least $PASSWORD_MIN characters.", DialogField.Second)) }
        }
        act(
            Busy.PASSWORD,
            SettingsSection.Password,
            inDialog = true,
            refused = { e ->
                when (e.status) {
                    401 -> DialogError("This is not your current password.")
                    400 -> DialogError("Use at least $PASSWORD_MIN characters.", DialogField.Second)
                    else -> null
                }
            },
        ) { api ->
            api.changePassword(ChangePasswordRequest(newPassword = next, oldPassword = current))
            "Password changed. Every device signs out, this TV too: it asks to be paired again soon."
        }
    }

    fun signOut() {
        if (mutable.value.busy != null) return
        mutable.update { it.copy(busy = Busy.SIGN_OUT) }
        viewModelScope.launch {
            // Best effort: the session is forgotten here whether the server heard or not.
            runCatching { api().logout() }
            container.sessionStore.clear()
            mutable.update { it.copy(busy = null, signedOut = true) }
        }
    }

    private companion object {
        const val CODE_LIFE_MS = 10 * 60_000L
        const val DEVICE_POLL_MS = 2_000L
    }
}

private suspend fun <A, B, C> coroutineScope3(
    a: suspend () -> A,
    b: suspend () -> B,
    c: suspend () -> C,
): Triple<A, B, C> = kotlinx.coroutines.coroutineScope {
    val da = async { a() }
    val db = async { b() }
    val dc = async { c() }
    Triple(da.await(), db.await(), dc.await())
}

/** The choices offered beside the preferences: none when they can't be read (the picks still save). */
private suspend fun <T> orNone(read: suspend () -> List<T>): List<T> =
    try {
        read()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        emptyList()
    }
