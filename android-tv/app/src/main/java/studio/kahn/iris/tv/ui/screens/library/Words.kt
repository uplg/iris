package studio.kahn.iris.tv.ui.screens.library

import studio.kahn.iris.tv.data.VIDEO_EXTENSIONS

// The library's own words; the shared ones are in `ui/format`.

/** A release name without its video extension (a single-file torrent is named after its file). */
fun releaseName(name: String): String {
    val ext = VIDEO_EXTENSIONS.firstOrNull { name.endsWith(it, ignoreCase = true) } ?: return name
    return name.dropLast(ext.length)
}

/** A file's own name, without its folders. */
fun fileName(path: String?): String? = path?.substringAfterLast('/')
