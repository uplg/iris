package studio.kahn.iris.tv.ui.screens.library

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.AvailableEpisodeEntry
import studio.kahn.iris.tv.data.EpisodeEntry
import studio.kahn.iris.tv.data.GoneEpisodeEntry
import studio.kahn.iris.tv.data.SeasonPackEntry
import studio.kahn.iris.tv.screenshot.LibraryFixtures as F
import studio.kahn.iris.tv.ui.components.StatusTone

class CollectionModelTest {
    private fun disk(s: Long, e: Long, lang: String? = "french", abs: Long? = null) =
        EpisodeEntry(episode = e, fileIdx = e, infohash = "ih$s", season = s, watched = false, absoluteEpisode = abs, language = lang)

    private fun offer(s: Long, e: Long, lang: String?, abs: Long? = null) = AvailableEpisodeEntry(
        episode = e, foundAt = F.at, indexerProvider = "torr9", indexerTorrentId = "$s$e$lang", season = s, absoluteEpisode = abs, language = lang,
    )

    private fun gone(s: Long, e: Long, lang: String?) = GoneEpisodeEntry(
        episode = e, fileIdx = 0, infohash = "g$s$e$lang", releaseName = "R", season = s, sourceExternalId = "x", sourceProvider = "p", watched = true, language = lang,
    )

    @Test
    fun mergeKeepsOneRowPerEpisodeAndDropsThePackSentinel() {
        val rows = mergeEpisodes(
            listOf(disk(1, 2), disk(1, 1), disk(1, 0)),
            listOf(offer(1, 2, "english"), offer(1, 3, "french")),
            listOf(gone(1, 1, "french"), gone(1, 1, "english")),
        )
        assertEquals(listOf("1-1", "1-2", "1-3"), rows.map { it.key })
        // a French copy on disk hides the French reclaimed one, not the English one
        val first = rows[0].variants
        assertTrue(first[0] is Variant.Downloaded)
        assertEquals(listOf("english"), first.filterIsInstance<Variant.Gone>().map { it.language })
        // on disk first, then offers
        assertTrue(rows[1].variants.first() is Variant.Downloaded)
        assertTrue(rows[1].variants.last() is Variant.Available)
    }

    @Test
    fun multiOnDiskHidesEveryReclaimedCopy() {
        val rows = mergeEpisodes(listOf(disk(1, 1, "multi")), reclaimed = listOf(gone(1, 1, "english")))
        assertTrue(rows[0].variants.none { it is Variant.Gone })
    }

    @Test
    fun absoluteLayoutLeavesSeasonCutOffersOut() {
        val rows = mergeEpisodesAbsolute(
            listOf(disk(1, 1156, abs = 1156), disk(23, 7, abs = null)),
            listOf(offer(1, 1157, "english", abs = 1157), offer(23, 8, "english", abs = null)),
        )
        assertEquals(listOf("a1156", "a1157", "23-7"), rows.map { it.key })
        assertEquals("Episode 1156", episodeName(rows[0]))
    }

    @Test
    fun seasonsComeFromEpisodesAndPacks() {
        val page = collectionPage(F.series, null, emptyList(), null, F.now)
        assertEquals(listOf(1L, 2L), page.seasons.map { it.season })
        assertEquals(1L, page.season)
        assertEquals("Season 1 · watched", page.seasons[0].label)
        assertTrue(page.packs.isEmpty())
        val two = collectionPage(F.series, null, emptyList(), 2L, F.now)
        assertEquals(1, two.packs.size)
        assertEquals("9 episodes · 6 on disk", two.seasonFact)
        assertEquals("Hello, Ms. Cobel", two.episodes.first().heading)
    }

    @Test
    fun aSeasonOfPacksAloneNeverSaysZeroEpisodes() {
        val pack = SeasonPackEntry(foundAt = F.at, indexerProvider = "torr9", indexerTorrentId = "p3", season = 3)
        val onDiskPack = disk(4, 0)
        val seasons = seasonsOf(mergeEpisodes(listOf(disk(1, 1), onDiskPack)), listOf(pack), listOf(disk(1, 1), onDiskPack))
        assertEquals(listOf(1L, 3L, 4L), seasons.map { it.season })
        assertEquals(listOf("Season 1 · 1 episode", "Season 3 · season pack", "Season 4 · season pack on disk"), seasons.map(::seasonLabel))

        val page = collectionPage(F.series.copy(episodes = F.series.episodes + onDiskPack), null, emptyList(), 4L, F.now)
        assertEquals("The season pack is on disk. Its episodes are not known one by one yet: play it from its files below.", page.emptyEpisodes)
        assertTrue(page.showFiles)
    }

    @Test
    fun rowStateSaysWhatToDo() {
        val rows = mergeEpisodes(F.series.episodes, F.series.availableEpisodes.orEmpty(), F.series.goneEpisodes.orEmpty(), F.series.episodeInfo)
        fun row(e: Long) = rows.first { it.season == 2L && it.episode == e }
        fun state(e: Long) = rowState(row(e), { null })
        assertEquals(RowState(StatusTone.Ok, "Watched · 50 min", verb = Verb.WatchAgain), state(1))
        assertEquals(RowState(StatusTone.Ok, "On disk", verb = Verb.Play), state(5))
        // the place comes with the episode, no progress read per release
        val started = F.series.episodes.map { if (it.season == 2L && it.episode == 5L) it.copy(positionSeconds = 600.0, durationSeconds = 1_800.0) else it }
        val five = mergeEpisodes(started, emptyList(), emptyList()).first { it.season == 2L && it.episode == 5L }
        assertEquals(RowState(StatusTone.Info, "In progress · 20 min left", 600f / 1_800f, Verb.Resume), rowState(five, { null }))
        assertEquals("Available · 2 releases · English, French audio", state(7).text)
        assertEquals("Removed from disk to free space", state(9).text)
    }

    @Test
    fun episodeActionsNameTheLanguage() {
        val rows = mergeEpisodes(F.series.episodes, F.series.availableEpisodes.orEmpty(), F.series.goneEpisodes.orEmpty())
        val seven = rows.first { it.season == 2L && it.episode == 7L }
        val labels = episodeActions(seven, rowState(seven, { null }), { null }).map { it.label }
        assertEquals(listOf("Grab and play in English", "Grab and play in French"), labels)
        val five = rows.first { it.season == 2L && it.episode == 5L }
        val fiveActions = episodeActions(five, rowState(five, { null }), { null })
        assertEquals(listOf("Play", "Mark as watched"), fiveActions.map { it.label })
    }

    @Test
    fun playLabelResumesOrStarts() {
        assertEquals("Start S1:E1", playLabel(F.series, null))
        val resume = F.watching[0].copy(infohash = "aa04", season = 2, episode = 4, positionSeconds = 1_930.0)
        assertEquals("Resume S2:E4 at 32:10", playLabel(F.series, resume))
        assertEquals("Play S2:E4", playLabel(F.series, resume.copy(nextUp = true)))
        assertEquals("Play", playLabel(F.movie, null))
    }

    @Test
    fun aSingleMovieCopyGoesStraightToThePlayer() {
        assertNull(straightToPlayer(F.movie))
        assertEquals(PlayTarget("aa03", 0), straightToPlayer(F.movie.copy(torrents = F.movie.torrents.take(1))))
        assertNull(straightToPlayer(F.series))
    }

    @Test
    fun releaseWordsFromTheName() {
        assertEquals("2160p · HEVC · HDR", qualityWords("Dune.Part.Two.2024.2160p.WEB-DL.DV.HDR.H265"))
        assertEquals("multi", nameLanguage("Dune.Part.Two.2024.MULTi.1080p"))
        assertEquals("french", nameLanguage("Film.2020.TRUEFRENCH.720p"))
        assertEquals("English audio (original)", audioChip("english", "en"))
        assertEquals("French audio (VF)", audioChip("french", "en"))
        assertEquals(listOf("fr", "en"), releaseCodes(listOf("french", "multi", null, "english", "french")))
    }

    @Test
    fun goneReleasesInlineAreNotRepeated() {
        val below = goneReleasesBelow(F.series)
        assertEquals(listOf("dd01"), below.map { it.infohash })
        assertEquals("Watched 30d ago", goneWatchLine(true, null, null, F.at.minusDays(30), F.now))
        assertEquals("Stopped at 10:00 (50%)", goneWatchLine(false, 600.0, 1_200.0, null, F.now))
    }
}
