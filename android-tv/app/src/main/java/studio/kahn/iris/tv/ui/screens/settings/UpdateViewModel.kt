package studio.kahn.iris.tv.ui.screens.settings

import android.app.Application
import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.BuildConfig
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.AppUpdater

@Immutable
data class UpdateUiState(
    val installed: String = BuildConfig.VERSION_NAME,
    val installedCode: Int = BuildConfig.VERSION_CODE,
    val latest: AppUpdater.VersionStatus = AppUpdater.VersionStatus.Unknown,
    /** Whether the sidecar version check is still running (it never blocks the download). */
    val checking: Boolean = true,
    val progress: AppUpdater.Progress? = null,
) {
    val downloading: Boolean
        get() = progress is AppUpdater.Progress.Connecting || progress is AppUpdater.Progress.Downloading
}

/**
 * The in-app update ([AppUpdater]): the sidecar version check on entry,
 * then the APK download. Handing the file to the installer is the
 * screen's (it needs the activity in front).
 */
class UpdateViewModel(
    private val container: AppContainer,
    private val app: Application,
) : ViewModel() {
    private val mutable = MutableStateFlow(UpdateUiState())
    val state: StateFlow<UpdateUiState> = mutable.asStateFlow()
    private var job: Job? = null

    init {
        viewModelScope.launch {
            val latest = AppUpdater.fetchLatestVersion(container.okHttpClient)
            mutable.update {
                it.copy(latest = AppUpdater.versionStatus(it.installed, latest), checking = false)
            }
        }
    }

    fun download() {
        if (job?.isActive == true) return
        mutable.update { it.copy(progress = AppUpdater.Progress.Connecting) }
        job = viewModelScope.launch {
            AppUpdater.downloadApk(app, container.okHttpClient).collect { p ->
                mutable.update { it.copy(progress = p) }
            }
        }
    }

    fun cancel() {
        job?.cancel()
        mutable.update { it.copy(progress = null) }
    }

    /** Android 8+ asks once for "Install unknown apps"; the system page was opened. */
    fun needsInstallPermission() {
        mutable.update {
            it.copy(progress = AppUpdater.Progress.Failed("Allow “Install unknown apps” for Iris TV in the page that opened, then try again."))
        }
    }
}
