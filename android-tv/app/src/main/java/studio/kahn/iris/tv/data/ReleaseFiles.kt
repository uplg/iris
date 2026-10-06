package studio.kahn.iris.tv.data

/**
 * The files of a torrent in the order one picks from (web `release/files.ts`):
 * videos first, episodes in order, then the biggest; and the file a grab
 * plays when nobody chose (episode 1 of a pack, else the biggest video).
 */
data class ReleaseFile(val index: Int, val path: String, val sizeBytes: Long, val isVideo: Boolean)

fun TorrentFilePreview.asReleaseFile() = ReleaseFile(index, path, sizeBytes, isVideo)

fun FileEntry.asReleaseFile() = ReleaseFile(index, path, sizeBytes, isVideoPath(path))

/** A season and episode a release or file name carries; a season alone (`S02`) is episode 0. */
data class SceneMark(val season: Int, val episode: Int)

// Seasons 1 or 2 digits, episodes up to 4 (long anime runs); the mark stands apart from
// letters and digits, so an underscore is a boundary (web `sceneEpisode`).
private val SCENE_MARK = Regex("""(?:^|[^a-z0-9])s(\d{1,2})(?:[._ -]*e(\d{1,4}))?(?![a-z0-9])""", RegexOption.IGNORE_CASE)

/** `S01E02`, `S1E2`, `S01.E02`, `Show_S01E02_1080p`; a bare `S02` is the whole season (episode 0). */
fun sceneMark(name: String): SceneMark? {
    val m = SCENE_MARK.find(name.substringAfterLast('/')) ?: return null
    return SceneMark(m.groupValues[1].toInt(), m.groupValues[2].toIntOrNull() ?: 0)
}

private fun episodeOf(path: String): SceneMark? = sceneMark(path)?.takeIf { it.episode > 0 }

private val SAMPLE = Regex("""\bsample\b""")

/** SCENE samples are videos never to play by default (the backend's `is_main_video_file`). */
fun isSample(path: String): Boolean {
    val p = path.lowercase()
    return "/sample/" in p || ".sample." in p || SAMPLE.containsMatchIn(p)
}

/** Each file's episode is read once, not on every comparison (a 100-file pack on a slow box). */
fun sortFiles(files: List<ReleaseFile>): List<ReleaseFile> = files
    .map { it to episodeOf(it.path) }
    .sortedWith { (a, sa), (b, sb) ->
        if (a.isVideo != b.isVideo) return@sortedWith if (a.isVideo) -1 else 1
        when {
            sa != null && sb != null -> compareValuesBy(sa, sb, SceneMark::season, SceneMark::episode)
            sa != null -> -1
            sb != null -> 1
            else -> b.sizeBytes.compareTo(a.sizeBytes)
        }
    }
    .map { it.first }

/** The videos one may choose to play (samples left out). */
fun playableFiles(files: List<ReleaseFile>): List<ReleaseFile> =
    sortFiles(files).filter { it.isVideo && !isSample(it.path) }

/**
 * The one rule for which file plays when nobody chose (web `autoFile`), for a grab, a release,
 * a title or the launcher: samples left out, a pack's first episode, else the biggest video.
 */
fun autoFile(files: List<ReleaseFile>): Int? {
    val videos = files.filter { it.isVideo && !isSample(it.path) }
    val pool = videos.ifEmpty { files }
    if (pool.isEmpty()) return null
    val episodes = pool.filter { episodeOf(it.path) != null }
    if (episodes.isNotEmpty()) return sortFiles(episodes).first().index
    return pool.maxBy { it.sizeBytes }.index
}

/** The file [t] plays ([autoFile] over its videos); null when it has none. */
fun playFileOf(t: TorrentView): Int? = autoFile(t.files.map { it.asReleaseFile() }.filter { it.isVideo })

/** The main action's words for the chosen file: which episode a pack starts with. */
fun playWords(files: List<ReleaseFile>, index: Int?): String {
    val episode = files.firstOrNull { it.index == index }?.let { episodeOf(it.path) }
    val videos = files.count { it.isVideo && !isSample(it.path) }
    return if (episode != null && videos > 1) "Download and play episode ${episode.episode}" else "Download and play"
}
