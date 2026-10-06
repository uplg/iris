package studio.kahn.iris.tv.ui.nav

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.UserResponse
import studio.kahn.iris.tv.ui.state.STOP_TIMEOUT_MS

/**
 * The signed-in person, for the header's account button: the server's
 * display name once read, the stored email until then. Read again after
 * pairing and after a change of name in Settings ([refresh]).
 */
class ShellViewModel(private val container: AppContainer) : ViewModel() {
    private val me = MutableStateFlow<UserResponse?>(null)
    private var reading: Job? = null

    val accountName: StateFlow<String> = combine(me, container.sessionStore.session) { user, session ->
        accountWords(user?.displayName, session?.email)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), accountWords(null, null))

    fun refresh() {
        reading?.cancel()
        reading = viewModelScope.launch {
            val url = container.sessionStore.serverUrl.first() ?: return@launch
            runCatching { container.apiFor(url).me() }.onSuccess { me.value = it }
        }
    }

    fun clear() {
        reading?.cancel()
        me.value = null
    }
}

/** The display name, else the email's name part, else "Account": the button always has words. */
internal fun accountWords(displayName: String?, email: String?): String =
    displayName?.trim()?.takeIf { it.isNotEmpty() }
        ?: email?.substringBefore('@')?.trim()?.takeIf { it.isNotEmpty() }
        ?: "Account"
