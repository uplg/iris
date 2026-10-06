package studio.kahn.iris.tv

import android.app.Application
import android.os.StrictMode
import coil3.ImageLoader
import coil3.PlatformContext
import coil3.SingletonImageLoader
import coil3.disk.DiskCache
import coil3.disk.directory
import coil3.memory.MemoryCache
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
        if (BuildConfig.DEBUG) enableStrictMode()
        super.onCreate()
        container = DefaultAppContainer(this)
    }

    // Sized for a 1-2 GB box: a fifth of the app heap for decoded posters
    // (cards request them at display size), a bounded disk cache so a long
    // browse never fills the box's small storage.
    override fun newImageLoader(context: PlatformContext): ImageLoader =
        ImageLoader.Builder(context)
            .components { add(SvgDecoder.Factory()) }
            .memoryCache {
                MemoryCache.Builder()
                    .maxSizePercent(context, IMAGE_MEMORY_FRACTION)
                    .build()
            }
            .diskCache {
                DiskCache.Builder()
                    .directory(context.cacheDir.resolve("image_cache"))
                    .maxSizeBytes(IMAGE_DISK_BYTES)
                    .build()
            }
            .build()

    private fun enableStrictMode() {
        StrictMode.setThreadPolicy(
            StrictMode.ThreadPolicy.Builder().detectAll().penaltyLog().build(),
        )
        StrictMode.setVmPolicy(
            StrictMode.VmPolicy.Builder().detectAll().penaltyLog().build(),
        )
    }

    private companion object {
        const val IMAGE_MEMORY_FRACTION = 0.2
        const val IMAGE_DISK_BYTES = 128L * 1024 * 1024
    }
}
