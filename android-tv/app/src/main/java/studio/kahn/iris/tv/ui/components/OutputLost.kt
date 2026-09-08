package studio.kahn.iris.tv.ui.components

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioManager
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat

/**
 * Fires [onLost] when the picture stops reaching the viewer without the
 * activity being stopped. A dongle whose remote switches the TV off over
 * infrared (no CEC) stays awake, and the player's `keepScreenOn` keeps it
 * that way: the lifecycle never moves and playback runs on behind a dark
 * screen for hours. What does arrive: the screen going off (a sleep the
 * lifecycle didn't stop us for), the HDMI audio sink dropping (the framework
 * counts HDMI among its "becoming noisy" outputs) and the HDMI hot-plug line
 * falling. Which one a given TV sends in standby varies; any of them means
 * nobody is watching.
 */
@Composable
fun OnOutputLost(onLost: () -> Unit) {
    val context = LocalContext.current
    val current = rememberUpdatedState(onLost)
    DisposableEffect(context) {
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(ctx: Context, intent: Intent) {
                val lost = when (intent.action) {
                    Intent.ACTION_SCREEN_OFF, AudioManager.ACTION_AUDIO_BECOMING_NOISY -> true
                    // Sticky: registering replays the current state, which is
                    // "plugged" while we're on screen. Only a drop counts.
                    ACTION_HDMI_PLUGGED ->
                        !intent.getBooleanExtra(EXTRA_HDMI_PLUGGED_STATE, true)
                    else -> false
                }
                if (lost) current.value()
            }
        }
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_SCREEN_OFF)
            addAction(AudioManager.ACTION_AUDIO_BECOMING_NOISY)
            addAction(ACTION_HDMI_PLUGGED)
        }
        // All three are protected broadcasts (only the system can send them),
        // so exporting the receiver exposes nothing; NOT_EXPORTED has been
        // seen to drop sticky system broadcasts on some OEM builds.
        ContextCompat.registerReceiver(context, receiver, filter, ContextCompat.RECEIVER_EXPORTED)
        onDispose { context.unregisterReceiver(receiver) }
    }
}

// `Intent.ACTION_HDMI_PLUGGED` and its extra are @hide, hence the literals.
private const val ACTION_HDMI_PLUGGED = "android.intent.action.HDMI_PLUGGED"
private const val EXTRA_HDMI_PLUGGED_STATE = "state"
