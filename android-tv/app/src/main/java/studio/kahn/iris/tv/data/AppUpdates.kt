package studio.kahn.iris.tv.data

import android.content.Context
import androidx.compose.runtime.Immutable
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import studio.kahn.iris.tv.BuildConfig

/** Where the in-app update stands: the version check, the download, a "Later" on the home. */
@Immutable
data class UpdateState(
    val installed: String = BuildConfig.VERSION_NAME,
    val installedCode: Int = BuildConfig.VERSION_CODE,
    val latest: AppUpdater.VersionStatus = AppUpdater.VersionStatus.Unknown,
    /** Whether the sidecar version check is running (it never blocks the download). */
    val checking: Boolean = false,
    val progress: AppUpdater.Progress? = null,
    /** The version the home notice was put off for ("Later"), until the app starts again. */
    val dismissed: String? = null,
) {
    val downloading: Boolean
        get() = progress is AppUpdater.Progress.Connecting || progress is AppUpdater.Progress.Downloading

    /** A newer version is hosted: the header says so, whatever the home notice does. */
    val available: String?
        get() = (latest as? AppUpdater.VersionStatus.UpdateAvailable)?.latest
}

/** The home's update notice: the versions ([latest] null when unknown), and the download. */
@Immutable
data class UpdateNotice(val latest: String?, val installed: String, val progress: AppUpdater.Progress?)

/**
 * The notice on the home: a newer version not put off for, or a download under way or done
 * (whatever started it, so it can be followed from there). A failed or unknown check shows
 * nothing: the version is not known to be newer.
 */
fun updateNotice(state: UpdateState): UpdateNotice? {
    val latest = state.available
    val wanted = latest != null && latest != state.dismissed
    val inFlight = state.downloading || state.progress is AppUpdater.Progress.Ready
    if (!wanted && !inFlight) return null
    return UpdateNotice(latest, state.installed, state.progress)
}

/**
 * The one in-app update of the process ([AppUpdater]): read by the header's badge, the home's
 * notice, Settings → App update and the update lock, so a download started on one is the
 * download the others show. Handing the file to the installer is the root's (it needs the
 * activity in front).
 */
class AppUpdates(
    private val context: Context,
    private val client: () -> OkHttpClient,
    private val scope: CoroutineScope,
) {
    private val mutable = MutableStateFlow(UpdateState())
    val state: StateFlow<UpdateState> = mutable.asStateFlow()
    private var check: Job? = null
    private var download: Job? = null

    /** Reads the hosted version, unless it is being read, or it is known and [force] is false. */
    fun check(force: Boolean = false) {
        if (check?.isActive == true) return
        if (!force && mutable.value.latest != AppUpdater.VersionStatus.Unknown) return
        mutable.update { it.copy(checking = true) }
        check = scope.launch {
            val latest = AppUpdater.fetchLatestVersion(client())
            mutable.update { it.copy(latest = AppUpdater.versionStatus(it.installed, latest), checking = false) }
        }
    }

    /** Downloads the APK; Android 8+ first asks once for "Install unknown apps" (a system page). */
    fun download() {
        if (download?.isActive == true) return
        if (!AppUpdater.canInstallPackages(context)) {
            AppUpdater.openInstallPermissionSettings(context)
            mutable.update {
                it.copy(progress = AppUpdater.Progress.Failed("Allow “Install unknown apps” for Iris TV in the page that opened, then try again."))
            }
            return
        }
        mutable.update { it.copy(progress = AppUpdater.Progress.Connecting) }
        download = scope.launch {
            AppUpdater.downloadApk(context, client()).collect { p -> mutable.update { it.copy(progress = p) } }
        }
    }

    /** Hands the downloaded APK to the system installer (again, when it was dismissed). */
    fun install() {
        val file = (mutable.value.progress as? AppUpdater.Progress.Ready)?.file ?: return
        AppUpdater.requestInstall(context, file)
    }

    fun cancel() {
        download?.cancel()
        mutable.update { it.copy(progress = null) }
    }

    /** "Later" on the home: no notice for this version until the app starts again. */
    fun later() {
        val latest = mutable.value.available ?: return
        mutable.update { s -> s.copy(dismissed = latest, progress = s.progress.takeUnless { it is AppUpdater.Progress.Failed }) }
    }
}
