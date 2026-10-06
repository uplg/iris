package studio.kahn.iris.tv.data

/** File extensions Iris plays; everything else in a torrent is extras. */
val VIDEO_EXTENSIONS = listOf(
    ".mkv", ".mp4", ".webm", ".m4v", ".avi", ".mov", ".ts", ".mts", ".m2ts", ".wmv",
)

fun isVideoPath(path: String): Boolean = VIDEO_EXTENSIONS.any { path.endsWith(it, ignoreCase = true) }
