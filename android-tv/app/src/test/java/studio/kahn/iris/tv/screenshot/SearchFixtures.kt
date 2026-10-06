package studio.kahn.iris.tv.screenshot

import java.time.OffsetDateTime
import java.time.ZoneOffset
import java.time.ZonedDateTime
import java.util.UUID
import studio.kahn.iris.tv.data.AudioInfo
import studio.kahn.iris.tv.data.DescriptionFormat
import studio.kahn.iris.tv.data.LibraryMatch
import studio.kahn.iris.tv.data.MediaInfoSummary
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.ProviderResultMeta
import studio.kahn.iris.tv.data.RecentSearchView
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.SubInfo
import studio.kahn.iris.tv.data.TitleCard
import studio.kahn.iris.tv.data.TitleMatch
import studio.kahn.iris.tv.data.TorrentDetails
import studio.kahn.iris.tv.data.TorrentFilePreview
import studio.kahn.iris.tv.data.TorrentPreview
import studio.kahn.iris.tv.data.VideoInfo
import studio.kahn.iris.tv.ui.screens.search.SearchPage
import studio.kahn.iris.tv.ui.screens.search.releaseNotes

/** The boards' Severance search, as the server would answer it (no artwork: no network in tests). */
object SearchFixtures {
    val now: ZonedDateTime = ZonedDateTime.of(2026, 10, 6, 21, 0, 0, 0, ZoneOffset.UTC)

    private fun at(daysAgo: Long, hour: Int = 20, minute: Int = 41): OffsetDateTime =
        now.minusDays(daysAgo).withHour(hour).withMinute(minute).toOffsetDateTime()

    val recent = listOf(
        RecentSearchView("the bear", at(0)),
        RecentSearchView("frieren", at(1)),
        RecentSearchView("anatomy of a fall", at(3)),
        RecentSearchView("dune 2024 multi", at(5)),
        RecentSearchView("shogun", at(8)),
    )

    private val severanceTv = TitleMatch(kind = MediaKind.tv, title = "Severance", tmdbId = 95396, year = 2022)
    private val severanceMovie = TitleMatch(kind = MediaKind.movie, title = "Severance", tmdbId = 9900, year = 2006)

    val titles = listOf(
        TitleCard(kind = MediaKind.tv, title = "Severance", tmdbId = 95396, collectionId = UUID(0, 1), year = 2022),
        TitleCard(kind = MediaKind.movie, title = "Severance", tmdbId = 9900, year = 2006),
        TitleCard(kind = MediaKind.movie, title = "Severed Ties", tmdbId = 41234, year = 1992),
        TitleCard(kind = MediaKind.tv, title = "Severance: Inside the Lumon", tmdbId = 77000, year = 2025),
    )

    val match = LibraryMatch(
        collectionId = "c-severance",
        displayTitle = "Severance",
        episodeCount = 17,
        isAnime = false,
        kind = "tv",
        torrentCount = 2,
    )

    private fun release(
        id: String,
        provider: String,
        name: String,
        tag: String?,
        seeders: Int,
        gb: Double,
        season: Int? = 2,
        episode: Int? = 0,
        codec: String? = "hevc",
        title: TitleMatch? = severanceTv,
        owned: Boolean = false,
        freeleech: Boolean = false,
    ) = SearchResult(
        externalId = id,
        providerId = provider,
        title = name,
        alreadyInLibrary = owned,
        codec = codec,
        freeleech = freeleech,
        kind = title?.kind,
        languageTag = tag,
        libraryInfohash = if (owned) "abc" else null,
        libraryFileIdx = if (owned) 0 else null,
        parsedSeason = season,
        parsedEpisode = episode,
        seeders = seeders,
        leechers = 9,
        sizeBytes = (gb * 1024 * 1024 * 1024).toLong(),
        titleMatch = title,
    )

    val releases = listOf(
        release("1", "torr9", "Severance.S02.MULTi.1080p.ATVP.WEB-DL.DDP5.1.HEVC", "multi", 142, 12.4, owned = true),
        release("2", "v3x", "Severance.S02.VOSTFR.1080p.WEB.H264", "vost", 61, 14.8, codec = "h264", freeleech = true),
        release("3", "torrentleech", "Severance.S02.2160p.ATVP.WEB-DL.DV.HDR.H265", "en", 33, 58.2),
        release("4", "torr9", "Severance.2006.MULTi.1080p.BluRay.x264", "multi", 27, 7.9, season = null, episode = null, codec = "h264", title = severanceMovie),
        release("5", "torr9", "Severance.S02E07.FRENCH.720p.WEB.x264", "fr", 18, 1.1, episode = 7, codec = "h264"),
        release("6", "v3x", "Severance.S02.FRENCH.1080p.HDLight.x264", "fr", 0, 9.6, codec = "h264"),
        release("7", "v3x", "Severance.S01.MULTi.1080p.WEB.H265", "multi", 88, 10.2, season = 1),
        release("8", "torr9", "Severance.S01.FRENCH.720p.WEB.x264", "fr", 40, 6.1, season = 1, codec = "h264"),
        release("9", "torrentleech", "Severance.S01.2160p.ATVP.WEB-DL.DV.H265", "en", 21, 49.0, season = 1),
        release("10", "torr9", "Severance.S02E10.MULTi.1080p.WEB.H265", "multi", 64, 1.6, episode = 10),
        release("11", "v3x", "Severance.2006.FRENCH.720p.BluRay.x264", "fr", 6, 4.4, season = null, episode = null, codec = "h264", title = severanceMovie),
        release("12", "torr9", "Severance.S02E09.MULTi.1080p.WEB.H265", "multi", 51, 1.5, episode = 9),
        release("13", "torrentleech", "Severance.S02E08.1080p.ATVP.WEB-DL.H265", "en", 39, 1.4, episode = 8),
        release("14", "v3x", "Severance.S01-S02.MULTi.1080p.WEB.H265", "multi", 25, 22.0, season = null, episode = null),
        release("15", "torr9", "Severance.S02E06.FRENCH.720p.WEB.x264", "fr", 12, 1.0, episode = 6, codec = "h264"),
        release("16", "torr9", "Severance.S02E05.FRENCH.720p.WEB.x264", "fr", 14, 1.0, episode = 5, codec = "h264"),
    )

    val providers = listOf(
        ProviderResultMeta(currentPage = 1, id = "torr9", limit = 25, totalCount = 40, totalPages = 3),
        ProviderResultMeta(currentPage = 1, id = "v3x", limit = 25, totalCount = 14, totalPages = 1),
        ProviderResultMeta(currentPage = 1, id = "torrentleech", limit = 25, totalCount = 9, totalPages = 1),
        ProviderResultMeta(currentPage = 1, id = "c411", limit = 25, error = "timeout"),
    )

    val page = SearchPage(rows = releases, providers = providers, matches = listOf(match), parsed = null, pages = 1, next = 2)

    val preview = TorrentPreview(
        announceUrls = emptyList(),
        files = (1..10).map { ep ->
            TorrentFilePreview(index = ep - 1, isArchive = false, isVideo = true, path = "Severance.S02.VOSTFR.1080p.WEB.H264/Severance.S02E%02d.mkv".format(ep), sizeBytes = 1_580_000_000L)
        } + TorrentFilePreview(index = 10, isArchive = false, isVideo = false, path = "Severance.S02.VOSTFR.1080p.WEB.H264/release.nfo", sizeBytes = 4_000),
        infohash = "def",
        name = "Severance.S02.VOSTFR.1080p.WEB.H264",
        pieceCount = 1,
        pieceLength = 1,
        streamable = true,
        totalSizeBytes = 15_800_004_000L,
    )

    private const val NOTES = "[center][size=150][color=#3d85c6][b]Severance, season 2, complete[/b][/color][/size][/center]\n" +
        "━━━━━━━━━━━━━━━━━━━━\n" +
        "10 episodes in 1080p from the Apple TV+ WEB-DL, re-encoded in [b]H.264[/b] for older players.\n\n" +
        "Audio: original English 5.1. Subtitles: full French and forced French for on-screen text, both checked against the episodes.\n\n" +
        "Episode 3 was replaced on 30 September after a sync issue in the last 5 minutes. [i]Thanks for seeding.[/i]\n\n" +
        "[img]https://example.org/banner.png[/img]\n" +
        "[table][tr][td]Source[/td][td]ATVP WEB-DL[/td][/tr][tr][td]Encoder[/td][td]x264[/td][/tr][/table]"

    val details = TorrentDetails(
        externalId = "2",
        providerId = "v3x",
        title = "Severance.S02.VOSTFR.1080p.WEB.H264",
        age = "1 week",
        description = NOTES,
        descriptionFormat = DescriptionFormat.bbcode,
        freeleech = true,
        leechers = 9,
        seeders = 61,
        mediaInfo = MediaInfoSummary(
            video = VideoInfo(codec = "AVC", resolution = "1920x1080", fps = 23.976f, durationSecs = 3300),
            audio = listOf(AudioInfo(lang = "English", commercialName = "Dolby Digital Plus", channels = 6)),
            subtitles = listOf(SubInfo(lang = "French", format = "UTF-8"), SubInfo(lang = "French", format = "UTF-8", forced = true)),
        ),
        nfo = (1..40).joinToString("\n") { "Line $it ........ Severance.S02E%02d  1920x1080  AVC  6400 kb/s".format((it % 10) + 1) },
    )

    val notes = releaseNotes(NOTES, DescriptionFormat.bbcode)
}
