package studio.kahn.iris.tv

import android.app.Application
import coil3.ImageLoader
import coil3.PlatformContext
import coil3.SingletonImageLoader
import coil3.svg.SvgDecoder
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.DefaultAppContainer

/**
 * Manual DI entrypoint. Hilt would be tempting but it's another KSP layer
 * to maintain — at this scale a single container plumbed through the
 * Application is plenty.
 *
 * Also configures the app-wide Coil [ImageLoader]: some channel logos are
 * SVG (a browser `<img>` renders them, but Coil needs an explicit decoder),
 * so register [SvgDecoder] — otherwise those logos silently fail to load.
 */
class IrisApp : Application(), SingletonImageLoader.Factory {
    lateinit var container: AppContainer
        private set

    override fun onCreate() {
        super.onCreate()
        container = DefaultAppContainer(this)
    }

    override fun newImageLoader(context: PlatformContext): ImageLoader =
        ImageLoader.Builder(context)
            .components { add(SvgDecoder.Factory()) }
            .build()
}
