package studio.kahn.iris.tv.screenshot

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import java.time.OffsetDateTime
import java.util.Locale
import java.util.UUID
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveCountry
import studio.kahn.iris.tv.data.LiveNowNext
import studio.kahn.iris.tv.data.LiveProgramme
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.ui.components.Step
import studio.kahn.iris.tv.ui.components.StepState
import studio.kahn.iris.tv.ui.screens.LiveBottomBar
import studio.kahn.iris.tv.ui.screens.LiveTopBar
import studio.kahn.iris.tv.ui.screens.live.LiveGuide
import studio.kahn.iris.tv.ui.screens.live.LiveTvContent
import studio.kahn.iris.tv.ui.screens.live.LiveTvUi
import studio.kahn.iris.tv.ui.screens.player.EpisodesPanel
import studio.kahn.iris.tv.ui.screens.player.GettingReadyContent
import studio.kahn.iris.tv.ui.screens.player.GettingReadyUi
import studio.kahn.iris.tv.ui.screens.player.GrabTarget
import studio.kahn.iris.tv.ui.screens.player.PlayerButtonFocus
import studio.kahn.iris.tv.ui.screens.player.PlayerButtons
import studio.kahn.iris.tv.ui.screens.player.PlayerControls
import studio.kahn.iris.tv.ui.screens.player.PlayerTitle
import studio.kahn.iris.tv.ui.screens.player.ReadyProblem
import studio.kahn.iris.tv.ui.screens.player.Readiness
import studio.kahn.iris.tv.ui.screens.player.keptForText
import studio.kahn.iris.tv.ui.screens.player.ScrubPosition
import studio.kahn.iris.tv.ui.screens.player.SideRow
import studio.kahn.iris.tv.ui.screens.player.TrackChoice
import studio.kahn.iris.tv.ui.screens.player.TrackMenu
import studio.kahn.iris.tv.ui.screens.player.TracksPanel
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.theme.IrisColor

/**
 * The player's chrome, panels and getting ready (TVPlayer, TVPlayerTracks,
 * TVPlayerStarting) and Live TV, over the stage. The video surface itself
 * does not render on the JVM: the stage color stands in for the picture.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = TV_QUALIFIERS)
class PlayerScreenshots {
    @get:Rule
    val shots = IrisScreenshotRule()

    @Composable
    private fun Stage(content: @Composable () -> Unit) {
        Box(Modifier.fillMaxSize().background(IrisColor.stageRaised)) { content() }
    }

    @Composable
    private fun Controls(nextLabel: String?, focusPlay: Boolean) {
        val focus = remember { PlayerButtonFocus() }
        if (focusPlay) LaunchedEffect(Unit) { runCatching { focus.play.requestFocus() } }
        Stage {
            PlayerControls(
                title = PlayerTitle("Severance", "S2:E4 · Woe's Hollow", "Playing from disk · 1080p HEVC"),
                clock = "21:47",
                scrub = ScrubPosition(positionMs = 1_930_000, bufferedMs = 2_700_000, durationMs = 3_300_000),
                buttons = PlayerButtons(
                    playing = true,
                    showTracks = true,
                    sideLabel = "Episodes",
                    nextLabel = nextLabel,
                    trailing = "English audio · English subtitles (SDH)",
                ),
                focus = focus,
                onPlayPause = {},
                onTracks = {},
                onSide = {},
                onNext = {},
                onLeaveButtons = {},
            )
        }
    }

    @Test
    fun controls() = shots.snapEverySize("player_controls") { Controls(nextLabel = "Next: Trojan's Horse", focusPlay = true) }

    @Test
    fun controlsPeek() = shots.snap("player_controls_peek") { Controls(nextLabel = null, focusPlay = false) }

    @Test
    fun tracks() = shots.snapEverySize("player_tracks") {
        Stage {
            TracksPanel(
                menu = TrackMenu(
                    audio = listOf(
                        TrackChoice("a:0", "English, original", selected = true),
                        TrackChoice("a:1", "French (VF)", selected = false),
                        TrackChoice("a:2", "English, audio description", selected = false),
                    ),
                    subtitles = listOf(
                        TrackChoice("s:off", "Off", selected = false),
                        TrackChoice("s:0", "English, for deaf and hard of hearing (SDH)", selected = true),
                        TrackChoice("s:1", "English, signs and songs only", selected = false),
                        TrackChoice("s:2", "French", selected = false),
                    ),
                    summary = "",
                ),
                keptFor = keptForText(UUID.randomUUID(), MediaKind.tv),
                onChoose = {},
                onDismiss = {},
            )
        }
    }

    @Test
    fun episodes() = shots.snapEverySize("player_episodes") {
        fun row(e: Long, name: String, watched: Boolean = false, pct: Double? = null, active: Boolean = false, grab: Boolean = false) = SideRow(
            key = "e$e",
            infohash = if (grab) "" else "ab",
            fileIdx = e.toInt(),
            primary = "S2:E$e · $name",
            secondary = "english",
            mono = false,
            watched = watched,
            watchedPct = pct,
            active = active,
            grab = if (grab) GrabTarget(2, e, "english") else null,
            season = 2,
            episode = e,
        )
        Stage {
            EpisodesPanel(
                title = "Episodes",
                rows = listOf(
                    row(1, "Hello, Ms. Cobel", watched = true),
                    row(2, "Goodbye, Mrs. Selvig", watched = true),
                    row(3, "Who Is Alive?", pct = 40.0),
                    row(4, "Woe's Hollow", active = true, pct = 58.0),
                    row(5, "Trojan's Horse"),
                    row(6, "Attila", grab = true),
                ),
                busyKey = null,
                onPlay = {},
                onGrab = {},
                onDismiss = {},
            )
        }
    }

    private val steps = listOf(
        Step("Connected to peers", StepState.Done, detail = "38 peers"),
        Step("Downloading the first minutes", StepState.Current, detail = "64 % · 8.2 MB/s", progress = 0.64f),
        Step("Read the video details", StepState.Pending),
        Step("Start playback", StepState.Pending),
    )

    private fun ready(problem: ReadyProblem? = null, gone: Boolean = false) = GettingReadyUi(
        title = "Severance",
        subtitle = "S2:E1 · Hello, Ms. Cobel · English audio, French subtitles",
        posterUrl = null,
        readiness = Readiness(steps, problem),
        gone = gone,
        note = "The rest of the season keeps downloading in the background.",
    )

    @Test
    fun gettingReady() = shots.snapEverySize("player_getting_ready") {
        GettingReadyContent(ready(), onCancel = {}, onPickAnother = {}, onRetry = {}, onRegrab = {})
    }

    @Test
    fun gettingReadyDeadSwarm() = shots.snap("player_getting_ready_dead") {
        GettingReadyContent(
            ready(
                ReadyProblem(
                    "Nobody is sharing this release",
                    "The tracker advertised seeders, but none of them answered. Iris has 12 % of the file and no way to get the rest.",
                    deadSwarm = true,
                ),
            ),
            onCancel = {},
            onPickAnother = {},
            onRetry = {},
            onRegrab = {},
        )
    }

    @Test
    fun gettingReadyGone() = shots.snap("player_getting_ready_gone") {
        GettingReadyContent(ready(gone = true), onCancel = {}, onPickAnother = {}, onRetry = {}, onRegrab = {})
    }

    private val start = OffsetDateTime.parse("2026-10-06T20:30:00Z")
    private val clock = { t: OffsetDateTime -> String.format(Locale.ROOT, "%02d:%02d", t.hour, t.minute) }
    private val news = LiveNowNext(
        channelId = "tf1",
        now = LiveProgramme(start = start, stop = start.plusMinutes(60), title = "Le journal de 20h30"),
        next = LiveProgramme(start = start.plusMinutes(60), stop = start.plusMinutes(150), title = "Koh-Lanta"),
    )

    private fun channel(id: String, name: String, tnt: Int? = null, cat: String? = null, geo: Boolean = false) =
        LiveChannel(categories = listOfNotNull(cat), geoBlocked = geo, id = id, name = name, not247 = false, tntNumber = tnt)

    @Test
    fun liveList() = shots.snapEverySize("live_list") {
        LiveTvContent(
            ui = LiveTvUi(
                countries = listOf(LiveCountry(code = "fr", flag = "FR", name = "France")),
                country = "fr",
                channels = Loadable.Ready(
                    listOf(
                        channel("tf1", "TF1", tnt = 1),
                        channel("f2", "France 2", tnt = 2),
                        channel("f3", "France 3", tnt = 3),
                        channel("c5", "France 5", tnt = 5),
                        channel("m6", "M6", tnt = 6),
                        channel("arte", "Arte", tnt = 7),
                        channel("bfm", "BFM TV", cat = "News"),
                        channel("cnn", "CNN International", cat = "News", geo = true),
                    ),
                ),
                guide = LiveGuide(mapOf("tf1" to news), start.plusMinutes(20).toInstant().toEpochMilli()),
                query = "",
                results = null,
            ),
            clock = clock,
            countryName = { "France" },
            onQueryChange = {},
            onPickCountry = {},
            onOpen = { _, _ -> },
            onRetry = {},
        )
    }

    @Test
    fun liveWatch() = shots.snapEverySize("live_watch") {
        val actions = remember { FocusRequester() }
        LaunchedEffect(Unit) { runCatching { actions.requestFocus() } }
        Stage {
            Box(Modifier.fillMaxSize()) {
                LiveTopBar(
                    channel = channel("tf1", "TF1", tnt = 1),
                    fallbackName = "tf1",
                    nowNext = news,
                    clock = clock,
                    modifier = Modifier.align(Alignment.TopStart),
                )
                LiveBottomBar(
                    nowNext = news,
                    nowMs = start.plusMinutes(20).toInstant().toEpochMilli(),
                    clock = clock,
                    actionsShown = true,
                    actionsFocus = actions,
                    onAnotherSource = {},
                    onChannels = {},
                    modifier = Modifier.align(Alignment.BottomStart),
                )
            }
        }
    }
}
