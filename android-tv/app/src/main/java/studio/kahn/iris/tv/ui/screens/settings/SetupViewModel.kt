package studio.kahn.iris.tv.ui.screens.settings

import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.IrisSession
import studio.kahn.iris.tv.data.LoginRequest
import studio.kahn.iris.tv.ui.screens.DEFAULT_SERVER_URL
import studio.kahn.iris.tv.ui.state.toUiError

@Immutable
data class SetupUiState(
    val serverUrl: String = DEFAULT_SERVER_URL,
    val email: String = "",
    val password: String = "",
    val signingIn: Boolean = false,
    val error: String? = null,
    val signedIn: Boolean = false,
) {
    val canSignIn: Boolean get() = serverUrl.isNotBlank() && email.isNotBlank() && password.isNotEmpty()
}

/**
 * Sign-in with email and password, the fallback to pairing by code (which
 * keeps the password off the TV). The session cookies land in the cookie
 * jar through the empty session saved first, as pairing does.
 */
class SetupViewModel(private val container: AppContainer) : ViewModel() {
    private val mutable = MutableStateFlow(SetupUiState())
    val state: StateFlow<SetupUiState> = mutable.asStateFlow()
    private var urlEdited = false

    init {
        viewModelScope.launch {
            val stored = container.sessionStore.serverUrl.first()
            if (stored != null && !urlEdited) mutable.update { it.copy(serverUrl = stored) }
        }
    }

    fun onServerUrlChange(url: String) {
        urlEdited = true
        mutable.update { it.copy(serverUrl = url.trim(), error = null) }
    }

    fun onEmailChange(email: String) = mutable.update { it.copy(email = email.trim(), error = null) }

    fun onPasswordChange(password: String) = mutable.update { it.copy(password = password, error = null) }

    fun signIn() {
        val s = mutable.value
        if (s.signingIn || !s.canSignIn) return
        mutable.update { it.copy(signingIn = true, error = null) }
        viewModelScope.launch {
            try {
                container.sessionStore.saveSession(
                    IrisSession(serverUrl = s.serverUrl, email = s.email, isAdmin = false, cookies = emptyList()),
                )
                val user = container.apiFor(s.serverUrl).login(LoginRequest(email = s.email, password = s.password))
                container.sessionStore.session.first()?.let { session ->
                    container.sessionStore.saveSession(session.copy(email = user.email, isAdmin = user.isAdmin))
                }
                mutable.update { it.copy(signingIn = false, password = "", signedIn = true) }
            } catch (e: Exception) {
                val error = e.toUiError()
                container.sessionStore.clear()
                mutable.update {
                    it.copy(signingIn = false, error = if (error.status == 401) WRONG_CREDENTIALS else error.message)
                }
            }
        }
    }

    private companion object {
        const val WRONG_CREDENTIALS = "This email and password don't match an Iris account on this server."
    }
}
