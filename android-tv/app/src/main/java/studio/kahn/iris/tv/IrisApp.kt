package studio.kahn.iris.tv

import android.app.Application
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.DefaultAppContainer

/**
 * Manual DI entrypoint. Hilt would be tempting but it's another KSP layer
 * to maintain — at this scale a single container plumbed through the
 * Application is plenty.
 */
class IrisApp : Application() {
    lateinit var container: AppContainer
        private set

    override fun onCreate() {
        super.onCreate()
        container = DefaultAppContainer(this)
    }
}
