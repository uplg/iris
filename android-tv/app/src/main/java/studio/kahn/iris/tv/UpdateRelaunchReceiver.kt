package studio.kahn.iris.tv

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Relaunch Iris right after an in-app update installs.
 *
 * `MY_PACKAGE_REPLACED` is delivered to the NEW version of the app the
 * moment the system installer finishes replacing it — the one point in
 * the update flow where we can bring the UI back without user action.
 * Best-effort by design: Android 10+ background-activity-launch
 * restrictions may swallow the `startActivity` on some devices, in
 * which case the user still has the installer's "Open" button (the
 * plain LAUNCHER category in the manifest is what un-greys it) or the
 * home-screen icon. Never throws — a failed relaunch must not crash
 * the freshly-installed build.
 */
class UpdateRelaunchReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        val pm = context.packageManager
        val launch = pm.getLeanbackLaunchIntentForPackage(context.packageName)
            ?: pm.getLaunchIntentForPackage(context.packageName)
            ?: return
        launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        runCatching { context.startActivity(launch) }
    }
}
