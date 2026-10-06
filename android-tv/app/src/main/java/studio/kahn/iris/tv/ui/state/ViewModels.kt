package studio.kahn.iris.tv.ui.state

import androidx.compose.runtime.Composable
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.createSavedStateHandle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import studio.kahn.iris.tv.data.AppContainer

/**
 * The ViewModel of a screen, built from the app's [AppContainer] (one cached
 * Retrofit per server, the stores, the application scope) and the
 * navigation entry's [SavedStateHandle] (route arguments land there).
 * Scoped to the nav back-stack entry: it survives recomposition and
 * configuration changes, and is cleared when the screen leaves the stack.
 *
 * ```
 * @Composable
 * fun TorrentsScreen(container: AppContainer, onBack: () -> Unit) {
 *     val vm = irisViewModel(container) { c, _ -> TorrentsViewModel(c) }
 *     val state by vm.state.collectAsStateWithLifecycle()
 *     TorrentsContent(state, onRetry = vm::refresh, onBack = onBack)
 * }
 * ```
 *
 * Pass [key] when one destination hosts several instances (one per
 * infohash, say); the default is one per back-stack entry.
 */
@Composable
inline fun <reified VM : ViewModel> irisViewModel(
    container: AppContainer,
    key: String? = null,
    noinline create: (AppContainer, SavedStateHandle) -> VM,
): VM = viewModel(
    key = key,
    factory = viewModelFactory {
        initializer { create(container, createSavedStateHandle()) }
    },
)
