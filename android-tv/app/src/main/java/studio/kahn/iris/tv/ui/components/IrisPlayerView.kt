@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package studio.kahn.iris.tv.ui.components

import android.content.Context
import android.view.KeyEvent
import android.view.View
import android.view.animation.AccelerateInterpolator
import android.view.animation.DecelerateInterpolator
import androidx.media3.common.Player
import androidx.media3.common.util.Util
import androidx.media3.ui.PlayerView

/**
 * Media3 `PlayerView` with the overlay behaviour of a streaming app:
 *
 * - The whole controller fades in and out as one layer. Media3's own
 *   choreography hides in two steps (main bar, then the progress bar two
 *   seconds later), which reads as the overlay hanging before it closes. We
 *   turn that animation off, own the show timeout, and animate the
 *   controller's alpha ourselves.
 * - D-pad centre with the overlay hidden toggles play/pause on the spot (and
 *   brings the overlay up), instead of only revealing the overlay and leaving
 *   the user to walk to the play button. With the overlay visible the centre
 *   key activates the focused control, as before.
 */
class IrisPlayerView(context: Context) : PlayerView(context) {
    private val controllerView: View?
        get() = findViewById(androidx.media3.ui.R.id.exo_controller)

    private val hideRunnable = Runnable { fadeOutAndHide() }
    private val playbackListener = object : Player.Listener {
        override fun onIsPlayingChanged(isPlaying: Boolean) {
            // Paused, buffering or ended: the overlay stays. Playing again
            // restarts the clock.
            if (isPlaying) scheduleHide() else removeCallbacks(hideRunnable)
        }
    }

    init {
        useController = true
        controllerAutoShow = true
        setControllerAnimationEnabled(false)
        // 0 = Media3 never auto-hides; `scheduleHide` does, after
        // `HIDE_AFTER_MS` of playback without a key press.
        controllerShowTimeoutMs = 0
        setControllerVisibilityListener(
            ControllerVisibilityListener { visibility ->
                if (visibility == View.VISIBLE) {
                    fadeIn()
                    scheduleHide()
                } else {
                    removeCallbacks(hideRunnable)
                }
            },
        )
    }

    override fun setPlayer(player: Player?) {
        this.player?.removeListener(playbackListener)
        super.setPlayer(player)
        player?.addListener(playbackListener)
    }

    /** True while a centre press we consumed is still held: its release must
     *  not reach the play/pause button the overlay just focused. */
    private var swallowCentreUp = false

    /**
     * Centre key with the overlay hidden: toggle play/pause and bring the
     * overlay up. Returns true when the key was consumed. Fed from the
     * activity through [PlayerKeyRouter] (the view itself holds no focus
     * while the overlay is hidden) and from [dispatchKeyEvent] for the cases
     * where it does.
     */
    fun onRemoteKey(event: KeyEvent): Boolean {
        val centre = when (event.keyCode) {
            KeyEvent.KEYCODE_DPAD_CENTER, KeyEvent.KEYCODE_ENTER,
            KeyEvent.KEYCODE_NUMPAD_ENTER, KeyEvent.KEYCODE_BUTTON_A -> true
            else -> false
        }
        if (!centre) return false
        if (event.action == KeyEvent.ACTION_UP && swallowCentreUp) {
            swallowCentreUp = false
            return true
        }
        if (isControllerFullyVisible || player == null) return false
        if (event.action == KeyEvent.ACTION_DOWN && event.repeatCount == 0) {
            Util.handlePlayPauseButtonAction(player)
            showController()
            swallowCentreUp = true
        }
        return true
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (onRemoteKey(event)) return true
        val wasVisible = isControllerFullyVisible
        val handled = super.dispatchKeyEvent(event)
        // Any key while the overlay is up keeps it up a while longer — and
        // cancels a fade-out already under way. A key that just brought the
        // overlay up is left alone: its fade-in is running.
        if (wasVisible && isControllerFullyVisible) {
            controllerView?.animate()?.cancel()
            controllerView?.alpha = 1f
            scheduleHide()
        }
        return handled
    }

    /** Fade the overlay away, then hide it. `super.hideController()` alone is
     *  the instant cut the visibility listener also sees. */
    override fun hideController() {
        if (!isControllerFullyVisible) return
        fadeOutAndHide()
    }

    private fun scheduleHide() {
        removeCallbacks(hideRunnable)
        val p = player ?: return
        if (p.isPlaying) postDelayed(hideRunnable, HIDE_AFTER_MS)
    }

    private fun fadeIn() {
        val view = controllerView ?: return
        view.animate().cancel()
        view.alpha = 0f
        view.animate()
            .alpha(1f)
            .setDuration(FADE_IN_MS)
            .setInterpolator(DecelerateInterpolator())
            .start()
    }

    private fun fadeOutAndHide() {
        removeCallbacks(hideRunnable)
        val view = controllerView
        if (view == null) {
            super.hideController()
            return
        }
        view.animate().cancel()
        view.animate()
            .alpha(0f)
            .setDuration(FADE_OUT_MS)
            .setInterpolator(AccelerateInterpolator())
            .withEndAction {
                super.hideController()
                // Ready for the next show; the view is GONE meanwhile.
                view.alpha = 1f
            }
            .start()
    }

    private companion object {
        const val HIDE_AFTER_MS = 4_000L
        const val FADE_IN_MS = 180L
        const val FADE_OUT_MS = 320L
    }
}
