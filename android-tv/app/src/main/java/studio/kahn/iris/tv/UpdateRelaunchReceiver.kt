package studio.kahn.iris.tv

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

 /**
  * Relaunch Iris right after an in-app update: `MY_PACKAGE_REPLACED`
  * is delivered to the new version the moment the install finishes.
  * Best-effort — Android 10+ may block the background start, in which
  * case the installer's "Open" button / home icon remain. Never throws.
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
