package studio.kahn.iris.tv.screenshot

import java.time.Instant
import java.time.OffsetDateTime
import java.time.ZoneOffset
import java.util.UUID
import studio.kahn.iris.tv.data.AvailableEpisodeEntry
import studio.kahn.iris.tv.data.CollectionDetail
import studio.kahn.iris.tv.data.CollectionListItem
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.DiskSpace
import studio.kahn.iris.tv.data.EpisodeEntry
import studio.kahn.iris.tv.data.EpisodeInfo
import studio.kahn.iris.tv.data.FileEntry
import studio.kahn.iris.tv.data.GoneEpisodeEntry
import studio.kahn.iris.tv.data.GoneReleaseEntry
import studio.kahn.iris.tv.data.HistoryItem
import studio.kahn.iris.tv.data.HomeSummary
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.SeasonPackEntry
import studio.kahn.iris.tv.data.TitleWatch
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.TorrentView

/** Fake server answers for the library screens: fixed ids and dates, no artwork (no network). */
object LibraryFixtures {
    val now: Instant = Instant.parse("2026-10-06T20:00:00Z")
    val at: OffsetDateTime = OffsetDateTime.ofInstant(now, ZoneOffset.UTC)
    private const val GB = 1024L * 1024 * 1024

    fun id(n: Int): UUID = UUID.fromString("00000000-0000-0000-0000-%012d".format(n))

    fun title(
        n: Int,
        name: String,
        kind: MediaKind = MediaKind.movie,
        anime: Boolean = false,
        episodes: Long = 0,
        size: Long = 4 * GB,
        torrents: Long = 1,
        ghost: Boolean = false,
        watch: TitleWatch? = null,
    ) = CollectionListItem(
        displayTitle = name,
        episodeCount = episodes,
        id = id(n),
        kind = kind,
        torrentCount = torrents,
        totalSizeBytes = size,
        ghost = ghost,
        isAnime = anime,
        watch = watch,
    )

    fun watch(
        infohash: String,
        position: Double,
        duration: Double? = 3_000.0,
        season: Long? = null,
        episode: Long? = null,
        completed: Boolean = false,
        watchedEpisodes: Long = 0,
        fileIdx: Long = 0,
    ) = TitleWatch(
        completed = completed,
        fileIdx = fileIdx,
        infohash = infohash,
        lastWatchedAt = at.minusHours(3),
        positionSeconds = position,
        watchedEpisodes = watchedEpisodes,
        durationSeconds = duration,
        episode = episode,
        season = season,
    )

    private val severanceWatch = watch("aa04", 1_200.0, season = 2, episode = 4, watchedEpisodes = 12, fileIdx = 4)

    fun torrent(
        infohash: String,
        name: String,
        collection: Int? = null,
        pct: Double = 100.0,
        state: TorrentState = TorrentState.live,
        peers: Int = 3,
        down: Long = 0,
        up: Long = 120_000,
        size: Long = 8 * GB,
        files: List<FileEntry> = listOf(FileEntry(0, "$name.mkv", size)),
        canDelete: Boolean = true,
        addedBy: String = "Leonard",
        provider: String? = "torr9",
    ) = TorrentView(
        downloadSpeedBps = down,
        fetchedAt = at,
        files = files,
        finished = pct >= 100,
        infohash = infohash,
        name = name,
        peers = peers,
        progressBytes = (size * pct / 100).toLong(),
        progressPct = pct,
        state = state,
        totalSizeBytes = size,
        uploadSpeedBps = up,
        uploadedBytes = 2 * GB,
        addedAt = at.minusDays(2),
        addedBy = id(900),
        addedByName = addedBy,
        id = id(800),
        tmdbVerified = true,
        uploadedBytesTotal = 12 * GB,
        canDelete = canDelete,
        collectionId = collection?.let(::id),
        downloadedBytesTotal = 8 * GB,
        sourceProvider = provider,
    )

    val titles = listOf(
        title(1, "Severance", MediaKind.tv, episodes = 19, size = 38 * GB, torrents = 2, watch = severanceWatch),
        title(2, "Dune: Part Two", size = 14 * GB, watch = watch("aa03", 9_000.0, 9_000.0, completed = true, watchedEpisodes = 1)),
        title(3, "The Bear", MediaKind.tv, episodes = 28, size = 22 * GB, watch = watch("aa01", 600.0, season = 4, episode = 1)),
        title(4, "Shōgun", MediaKind.tv, episodes = 10, size = 30 * GB),
        title(5, "Perfect Days", size = 6 * GB, watch = watch("aa05", 2_400.0, 7_400.0)),
        title(6, "Frieren", MediaKind.tv, anime = true, episodes = 28, size = 18 * GB),
        title(7, "Andor", MediaKind.tv, episodes = 24, size = 40 * GB),
        title(8, "Anatomy of a Fall", size = 9 * GB),
        title(9, "Arcane", MediaKind.tv, episodes = 9, size = 12 * GB),
        title(10, "Past Lives", ghost = true),
        title(11, "Slow Horses", MediaKind.tv, episodes = 30, watch = watch("ff11", 2_900.0, season = 3, episode = 2, completed = true, watchedEpisodes = 14)),
        title(12, "The Zone of Interest"),
        title(13, "Fallout", MediaKind.tv, episodes = 8, watch = watch("ff13", 3_000.0, season = 1, episode = 8, completed = true, watchedEpisodes = 8)),
        title(14, "Aftersun"),
    )

    val torrents = listOf(
        torrent("aa01", "The.Bear.S04.1080p.WEB.H264", collection = 3, pct = 42.0, down = 6_400_000, peers = 18),
        torrent("aa02", "Arcane.S02.MULTi.1080p.WEB.x265", collection = 9, pct = 61.0, state = TorrentState.live, peers = 0, down = 0, canDelete = false, addedBy = "Camille"),
        torrent("aa03", "Dune.Part.Two.2024.2160p.WEB-DL.DV.HDR.H265", collection = 2, size = 14 * GB),
        torrent(
            "aa04",
            "Severance.S02.MULTi.1080p.ATVP.WEB-DL.HEVC",
            collection = 1,
            files = (1..10).map { FileEntry(it, "Severance.S02E%02d.1080p.mkv".format(it), 2 * GB) },
        ),
        torrent("aa05", "Perfect.Days.2023.FRENCH.1080p.WEB", collection = 5, state = TorrentState.paused, provider = "nyaa"),
    )

    val watching = listOf(
        ContinueWatchingItem(
            completed = false,
            fileIdx = 0,
            grabbable = false,
            infohash = "aa05",
            lastWatchedAt = at.minusHours(3),
            nextUp = false,
            positionSeconds = 2_400.0,
            tmdbVerified = true,
            torrentName = "Perfect.Days.2023.FRENCH.1080p.WEB",
            collectionId = id(5),
            durationSeconds = 7_400.0,
        ),
    )

    val summary = HomeSummary(
        downloading = 2,
        downloadingPct = 48.0,
        newEpisodes = 2,
        seeding = 3,
        disk = DiskSpace(freeBytes = 412 * GB, totalBytes = 2_000 * GB),
        downloadingEtaSeconds = 780,
    )

    private fun ep(s: Long, e: Long, ih: String, watched: Boolean = false, lang: String? = "multi") =
        EpisodeEntry(episode = e, fileIdx = e, infohash = ih, season = s, watched = watched, language = lang)

    val series = CollectionDetail(
        displayTitle = "Severance",
        episodes = (1L..6L).map { ep(2, it, "aa04", watched = it < 4) } + (1L..9L).map { ep(1, it, "bb01", watched = true) },
        id = id(1),
        kind = MediaKind.tv,
        torrents = listOf(
            torrents[3],
            torrent("bb01", "Severance.S01.1080p.WEB.H264", collection = 1, canDelete = false, addedBy = "Camille"),
        ),
        availableEpisodes = listOf(
            AvailableEpisodeEntry(episode = 7, foundAt = at, indexerProvider = "torr9", indexerTorrentId = "t7", season = 2, language = "french", quality = "1080p", seeders = 42, sizeBytes = 2 * GB),
            AvailableEpisodeEntry(episode = 7, foundAt = at, indexerProvider = "v3x", indexerTorrentId = "v7", season = 2, language = "english", quality = "2160p", seeders = 12, sizeBytes = 5 * GB),
            AvailableEpisodeEntry(episode = 8, foundAt = at, indexerProvider = "torr9", indexerTorrentId = "t8", season = 2, language = "english", quality = "1080p", seeders = 30, sizeBytes = 2 * GB),
        ),
        episodeInfo = listOf(
            EpisodeInfo(episode = 1, season = 2, name = "Hello, Ms. Cobel", runtimeMinutes = 50, airDate = "2025-01-17", overview = "Mark returns to Lumon after five months."),
            EpisodeInfo(episode = 4, season = 2, name = "Woe's Hollow", runtimeMinutes = 48),
            EpisodeInfo(episode = 7, season = 2, name = "Chikhai Bardo", runtimeMinutes = 55),
        ),
        goneEpisodes = listOf(
            GoneEpisodeEntry(episode = 9, fileIdx = 0, infohash = "cc09", releaseName = "Severance.S02E09.1080p", season = 2, sourceExternalId = "x9", sourceProvider = "torr9", watched = false, language = "french", quality = "1080p", totalSizeBytes = 2 * GB),
        ),
        goneReleases = listOf(
            GoneReleaseEntry(infohash = "dd01", name = "Severance.S01.2160p.ATVP.WEB-DL", sourceExternalId = "y1", sourceProvider = "v3x", totalSizeBytes = 48 * GB, deletedAt = at.minusDays(9), watched = true, lastWatchedAt = at.minusDays(30)),
        ),
        hasNewSinceLastVisit = 2,
        numbering = "seasonal",
        onWatchlist = true,
        normalizedName = "severance",
        watch = severanceWatch,
        seasonPacks = listOf(
            SeasonPackEntry(foundAt = at, indexerProvider = "torr9", indexerTorrentId = "p2", season = 2, language = "french", quality = "1080p", seeders = 88, sizeBytes = 21 * GB),
        ),
    )

    val movie = CollectionDetail(
        displayTitle = "Dune: Part Two",
        episodes = emptyList(),
        id = id(2),
        kind = MediaKind.movie,
        watch = watch("aa03", 9_000.0, 9_000.0, completed = true, watchedEpisodes = 1),
        torrents = listOf(torrents[2], torrent("aa06", "Dune.Part.Two.2024.MULTi.1080p.BluRay.x264", collection = 2, canDelete = false, addedBy = "Camille")),
    )

    val history = listOf(
        HistoryItem(
            completed = false, deleted = false, fileIdx = 4, infohash = "aa04", lastWatchedAt = at.minusHours(2), positionSeconds = 1_200.0,
            tmdbVerified = true, torrentName = "Severance.S02", collectionId = id(1), collectionTitle = "Severance", durationSeconds = 3_000.0,
            episode = 4, season = 2,
        ),
        HistoryItem(
            completed = true, deleted = false, fileIdx = 3, infohash = "aa04", lastWatchedAt = at.minusDays(1), positionSeconds = 3_000.0,
            tmdbVerified = true, torrentName = "Severance.S02", collectionId = id(1), collectionTitle = "Severance", durationSeconds = 3_000.0,
            episode = 3, season = 2,
        ),
        HistoryItem(
            completed = false, deleted = true, fileIdx = 0, infohash = "ee01", lastWatchedAt = at.minusDays(3), positionSeconds = 3_100.0,
            tmdbVerified = true, torrentName = "Past.Lives.2023.1080p.WEB", collectionId = id(10), collectionTitle = "Past Lives",
            durationSeconds = 6_300.0, sourceProvider = "torr9", sourceExternalId = "z1", kind = MediaKind.movie,
        ),
        HistoryItem(
            completed = true, deleted = false, fileIdx = 0, infohash = "aa03", lastWatchedAt = at.minusDays(12), positionSeconds = 9_000.0,
            tmdbVerified = true, torrentName = "Dune.Part.Two.2024.2160p", collectionId = id(2), collectionTitle = "Dune: Part Two",
            durationSeconds = 9_000.0, kind = MediaKind.movie,
        ),
    )
}
