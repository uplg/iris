package studio.kahn.iris.tv.ui.components

import android.view.KeyEvent

/**
 * First look at every hardware key of the activity window, for the screen that
 * owns playback. Keys reach a `PlayerView` only while something inside it holds
 * focus, and with the overlay hidden nothing does: the D-pad then lands
 * wherever Compose left its focus, and the player never hears about it. The
 * activity hands each key here before anything else; the watch screen installs
 * its player while mounted and removes it on the way out.
 */
object PlayerKeyRouter {
    @Volatile
    var handler: ((KeyEvent) -> Boolean)? = null
}
