package studio.kahn.iris.tv

import android.annotation.SuppressLint
import android.content.Intent
import android.os.Bundle
import android.view.KeyEvent
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.InterfaceSize
import studio.kahn.iris.tv.data.bestEffort
import studio.kahn.iris.tv.ui.IrisRoot
import studio.kahn.iris.tv.ui.components.PlayerKeyRouter
import studio.kahn.iris.tv.ui.nav.LaunchTarget
import studio.kahn.iris.tv.ui.nav.launchTarget
import studio.kahn.iris.tv.ui.theme.InterfaceScale
import studio.kahn.iris.tv.ui.theme.IrisTheme

class MainActivity : ComponentActivity() {
    /** The pending request from outside: a launcher deep link, voice search, the search key. */
    private val launch = MutableStateFlow<LaunchTarget?>(null)

    // Activity.dispatchKeyEvent is public framework API; lint trips on the
    // @RestrictTo that core's ComponentActivity puts on its own override.
    @SuppressLint("RestrictedApi")
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (PlayerKeyRouter.handler?.invoke(event) == true) return true
        if (event.keyCode == KeyEvent.KEYCODE_SEARCH) {
            if (event.action == KeyEvent.ACTION_UP) launch.value = LaunchTarget.Search()
            return true
        }
        return super.dispatchKeyEvent(event)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Edge-to-edge on every OS version so the insets reach Compose the same
        // way everywhere (IrisRoot pads with them; all zero on a TV).
        enableEdgeToEdge()
        val container = (application as IrisApp).container
        if (savedInstanceState == null) launch.value = intent.launchTarget()
        lifecycleScope.launch { bestEffort { container.channels.sync(container) } }
        setContent {
            // null until the stored session is read, so the first screen is
            // the right one. Pairing pre-seeds a session with no cookies for
            // the cookie jar to fill: that one is not signed in yet.
            val authenticated by produceState<Boolean?>(null) {
                container.sessionStore.session.collect { value = it?.cookies?.isNotEmpty() == true }
            }
            // null until read too: the first frame is drawn at its size, not resized after.
            val size by produceState<InterfaceSize?>(null) {
                container.prefsStore.interfaceSize.collect { value = it }
            }
            val pending by launch.collectAsStateWithLifecycle()
            val signedIn = authenticated
            val scale = size
            if (signedIn != null && scale != null) {
                InterfaceScale(scale) {
                    IrisTheme {
                        IrisRoot(
                            container = container,
                            isAuthenticated = signedIn,
                            launch = pending,
                            onLaunchHandled = { launch.value = null },
                        )
                    }
                }
            }
        }
    }

    // Leaving for the launcher: its row shows what was just watched (singleTask: onCreate is rare).
    override fun onStop() {
        super.onStop()
        val container = (application as IrisApp).container
        container.applicationScope.launch { bestEffort { container.channels.sync(container) } }
    }

    // singleTask: a Watch Next pick or a voice search while Iris is open lands here.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        intent.launchTarget()?.let { launch.value = it }
    }
}
