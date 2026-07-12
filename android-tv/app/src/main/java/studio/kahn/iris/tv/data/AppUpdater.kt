package studio.kahn.iris.tv.data

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import androidx.core.net.toUri
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.withContext
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
 * The fixed URLs are intentional — the user self-hosts the APK on
 * `uplg.xyz` and rebuilds out-of-band. Alongside the APK we also
 * publish a plain-text `app-release.version` sidecar containing just
 * the semver of what's hosted (one line, e.g. `0.2.0`). Whenever the
 * APK is replaced, this file is replaced too — that's how the
 * Settings screen knows whether the user is running the latest
 * build.
 */
object AppUpdater {

    /** Where the latest APK is hosted. */
    const val APK_URL: String = "https://uplg.xyz/app-release.apk"

    /** Sidecar plain-text file containing only the semver of the
     *  APK at [APK_URL] (e.g. `0.2.0\n`). Replaced atomically with
     *  the APK on every release. Missing / unreachable = the
     *  Settings card shows "version check unavailable" but the
     *  Download button still works (best-effort, never block the
     *  update path). */
    const val LATEST_VERSION_URL: String = "https://uplg.xyz/app-release.version"

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
                    // Throttled progress: one emission per 64 KiB chunk
                    // meant thousands of recompositions on the Main
                    // collector — enough to make slow boxes janky right
                    // when the installer intent fires. Every 512 KiB is
                    // still a smooth bar.
                    var lastEmitted = 0L
                    while (true) {
                        val n = source.read(buf)
                        if (n <= 0) break
                        out.write(buf, 0, n)
                        copied += n
                        if (copied - lastEmitted >= 512 * 1024 || copied == total) {
                            lastEmitted = copied
                            emit(Progress.Downloading(copied, total))
                        }
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
     * Best-effort fetch of the version string from
     * [LATEST_VERSION_URL]. Returns `null` when the request fails,
     * the body is empty, or the body doesn't look like a semver —
     * callers treat that as "unknown" and show the installed
     * version only.
     *
     * Runs on [Dispatchers.IO]; cancellable.
     */
    suspend fun fetchLatestVersion(client: OkHttpClient): String? =
        withContext(Dispatchers.IO) {
            try {
                val req = Request.Builder().url(LATEST_VERSION_URL).get().build()
                client.newCall(req).execute().use { resp ->
                    if (!resp.isSuccessful) return@withContext null
                    val raw = resp.body.string().trim()
                    raw.takeIf { SEMVER_REGEX.matches(it) }
                }
            } catch (_: IOException) {
                null
            }
        }

    /**
     * Compare two semver strings. Negative when [installed] is older
     * than [available], positive when newer, zero when equal. A
     * release (no pre-release tag) is treated as greater than the
     * same version with a pre-release tag (`0.2.0` > `0.2.0-rc1`).
     *
     * Robust to short inputs (`0.2` is treated as `0.2.0`) and to
     * non-numeric components (sorted lexicographically as a last
     * resort). Returns `0` only on exact equality; never `null`.
     */
    fun compareSemver(installed: String, available: String): Int {
        val (aCore, aPre) = splitPrerelease(installed)
        val (bCore, bPre) = splitPrerelease(available)
        val aParts = aCore.split('.').map { it.toIntOrNull() ?: 0 }
        val bParts = bCore.split('.').map { it.toIntOrNull() ?: 0 }
        for (i in 0 until maxOf(aParts.size, bParts.size)) {
            val ai = aParts.getOrNull(i) ?: 0
            val bi = bParts.getOrNull(i) ?: 0
            if (ai != bi) return ai.compareTo(bi)
        }
        return when {
            aPre == null && bPre == null -> 0
            aPre == null -> 1   // release > prerelease
            bPre == null -> -1
            else -> aPre.compareTo(bPre)
        }
    }

    /** Up-to-date / outdated verdict for the Settings card. */
    fun versionStatus(installed: String, available: String?): VersionStatus {
        val latest = available ?: return VersionStatus.Unknown
        return when {
            compareSemver(installed, latest) < 0 -> VersionStatus.UpdateAvailable(latest)
            else -> VersionStatus.UpToDate(latest)
        }
    }

    sealed interface VersionStatus {
        data object Unknown : VersionStatus
        data class UpToDate(val latest: String) : VersionStatus
        data class UpdateAvailable(val latest: String) : VersionStatus
    }

    private val SEMVER_REGEX = Regex("""\d+(?:\.\d+){0,2}(?:-[\w.]+)?""")

    private fun splitPrerelease(s: String): Pair<String, String?> {
        val idx = s.indexOf('-')
        return if (idx < 0) s to null else s.substring(0, idx) to s.substring(idx + 1)
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
                "package:${context.packageName}".toUri(),
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
