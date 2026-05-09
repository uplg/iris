package studio.kahn.iris.tv.data

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOn
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException

/**
 * In-app APK updater. Downloads the released APK from a fixed URL into
 * the app's cache, then hands it to the system package installer via
 * `Intent.ACTION_VIEW` + `FileProvider`. The system shows its own
 * "Update Iris TV?" confirmation; we never silently replace anything.
 *
 * The fixed URL is intentional — the user hosts the APK and rebuilds
 * out-of-band. A future nicety would be to compare a `<version>.txt`
 * sidecar against `BuildConfig.VERSION_NAME` to gate the button when
 * already up-to-date, but for now the user just clicks when they want.
 */
object AppUpdater {

    /** Where the latest APK is hosted. */
    const val APK_URL: String = "https://uplg.xyz/app-release.apk"

    /** Cache subdirectory used by [downloadApk]; cleared on each
     *  successful install request. */
    private const val CACHE_SUBDIR = "updates"
    private const val APK_FILENAME = "iris-tv-latest.apk"

    /** Progress events emitted while downloading the APK. */
    sealed interface Progress {
        data object Connecting : Progress
        /** `bytes` is the running total downloaded; `total` is the
         *  Content-Length header (`-1` when the server didn't set one,
         *  in which case the UI should show an indeterminate bar). */
        data class Downloading(val bytes: Long, val total: Long) : Progress
        /** APK has been written to disk; caller should now call
         *  [requestInstall] with the same [file] to launch the system
         *  package installer. */
        data class Ready(val file: File) : Progress
        data class Failed(val message: String) : Progress
    }

    /**
     * Stream the APK from [APK_URL] into the cache and emit progress
     * events. Cancellation by the caller (e.g. composable leaves
     * composition) cancels the underlying network read.
     */
    fun downloadApk(
        context: Context,
        client: OkHttpClient,
    ): Flow<Progress> = flow {
        emit(Progress.Connecting)
        val cacheDir = File(context.cacheDir, CACHE_SUBDIR).apply { mkdirs() }
        val target = File(cacheDir, APK_FILENAME)
        // Atomic-ish write via a sibling tmp file — protects us from
        // serving a half-downloaded APK to the package installer if a
        // previous attempt was interrupted.
        val tmp = File(cacheDir, "$APK_FILENAME.part")
        if (tmp.exists()) tmp.delete()

        val request = Request.Builder().url(APK_URL).get().build()
        val response = try {
            client.newCall(request).execute()
        } catch (e: IOException) {
            emit(Progress.Failed("network: ${e.message ?: "unreachable"}"))
            return@flow
        }
        response.use { resp ->
            if (!resp.isSuccessful) {
                emit(Progress.Failed("server returned HTTP ${resp.code}"))
                return@flow
            }
            // OkHttp 4+ guarantees `body` is non-null after a successful
            // response check; `contentLength()` returns -1 when the
            // server didn't set Content-Length.
            val body = resp.body
            val total = body.contentLength()
            val source = body.byteStream()
            try {
                tmp.outputStream().use { out ->
                    val buf = ByteArray(64 * 1024)
                    var copied = 0L
                    while (true) {
                        val n = source.read(buf)
                        if (n <= 0) break
                        out.write(buf, 0, n)
                        copied += n
                        emit(Progress.Downloading(copied, total))
                    }
                }
            } catch (e: IOException) {
                tmp.delete()
                emit(Progress.Failed("download interrupted: ${e.message ?: "io"}"))
                return@flow
            }
        }

        if (target.exists()) target.delete()
        if (!tmp.renameTo(target)) {
            tmp.delete()
            emit(Progress.Failed("could not rename downloaded apk to ${target.path}"))
            return@flow
        }
        emit(Progress.Ready(target))
    }.flowOn(Dispatchers.IO)

    /**
     * Returns true if we already have permission to install APKs from
     * this app, false if the user must first grant it via Settings →
     * Apps → Iris TV → Install unknown apps. Always true on Android <
     * 8 (the per-app permission was introduced in O).
     */
    fun canInstallPackages(context: Context): Boolean {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.packageManager.canRequestPackageInstalls()
        } else {
            true
        }
    }

    /**
     * Open the system Settings page where the user can grant
     * "Install unknown apps" for our package. Required once before
     * [requestInstall] succeeds on Android 8+.
     */
    fun openInstallPermissionSettings(context: Context) {
        val intent = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Intent(
                Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                Uri.parse("package:${context.packageName}"),
            )
        } else {
            Intent(Settings.ACTION_SECURITY_SETTINGS)
        }
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(intent)
    }

    /**
     * Hand the downloaded APK to the system package installer. The
     * URI is exported via `FileProvider` (see AndroidManifest +
     * `res/xml/file_provider_paths.xml`); the temporary read grant in
     * the intent flags is what lets the installer process actually
     * read the file across the package boundary.
     */
    fun requestInstall(context: Context, apk: File) {
        val authority = "${context.packageName}.fileprovider"
        val uri: Uri = FileProvider.getUriForFile(context, authority, apk)
        val install = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        context.startActivity(install)
    }
}
