package studio.kahn.iris.tv.screenshot

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.background
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.Subtitles
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Artwork
import studio.kahn.iris.tv.ui.components.CardRow
import studio.kahn.iris.tv.ui.components.Chip
import studio.kahn.iris.tv.ui.components.ChipSize
import studio.kahn.iris.tv.ui.components.ChipTone
import studio.kahn.iris.tv.ui.components.ConfirmDialog
import studio.kahn.iris.tv.ui.components.EmptyState
import studio.kahn.iris.tv.ui.components.ErrorState
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.FactRow
import studio.kahn.iris.tv.ui.components.FramedBlock
import studio.kahn.iris.tv.ui.components.IconAction
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.KeyHints
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.LanguageChip
import studio.kahn.iris.tv.ui.components.LoadingState
import studio.kahn.iris.tv.ui.components.Meter
import studio.kahn.iris.tv.ui.components.PanelLabel
import studio.kahn.iris.tv.ui.components.PanelOption
import studio.kahn.iris.tv.ui.components.PanelOptions
import studio.kahn.iris.tv.ui.components.Pill
import studio.kahn.iris.tv.ui.components.PillChoice
import studio.kahn.iris.tv.ui.components.PosterCard
import studio.kahn.iris.tv.ui.components.PosterGrid
import studio.kahn.iris.tv.ui.components.RowCard
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.SidePanel
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.Step
import studio.kahn.iris.tv.ui.components.StepList
import studio.kahn.iris.tv.ui.components.StepState
import studio.kahn.iris.tv.ui.components.StillCard
import studio.kahn.iris.tv.ui.components.TextInput
import studio.kahn.iris.tv.ui.components.TopTab
import studio.kahn.iris.tv.ui.components.TvHeader
import studio.kahn.iris.tv.ui.state.UiError
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** One gallery per shared component, at TV size; focused variants where focus changes the look. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = TV_QUALIFIERS)
class ComponentScreenshots {
    @get:Rule
    val shots = IrisScreenshotRule()

    @Composable
    private fun Page(content: @Composable () -> Unit) {
        Column(
            Modifier
                .fillMaxSize()
                .padding(IrisLayout.current.safePadding),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s7),
        ) { content() }
    }

    @Test
    fun typeAndColor() = shots.snap("theme_type_color") {
        Page {
            Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                Text("Severance", style = IrisType.hero)
                Text("Severance · Season 2, complete", style = IrisType.title)
                Text("Continue watching", style = IrisType.section)
                Text("Mark and his team push deeper into Lumon.", style = IrisType.body)
                Text("Season 2 · Episode 4 · 23 min left · 32:10", style = IrisType.meta, color = IrisColor.inkMuted)
                Text("Severance.S02.VOSTFR.1080p.WEB.H264", style = IrisType.mono, color = IrisColor.inkMuted)
                Eyebrow("Continue where you left off")
            }
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
                listOf(
                    IrisColor.ground, IrisColor.groundRaised, IrisColor.surface, IrisColor.line,
                    IrisColor.ink, IrisColor.inkMuted, IrisColor.accent, IrisColor.accentWash,
                    IrisColor.warn, IrisColor.down, IrisColor.stage, IrisColor.art,
                ).forEach { Box(Modifier.size(40.dp).background(it)) }
            }
        }
    }

    @Test
    fun header() {
        shots.snap("header") {
            Page { TvHeader(current = TopTab.Home, onSelect = {}, accountName = "Leonard", onAccount = {}) }
        }
        shots.snap("header_focused") {
            Page {
                TvHeader(current = TopTab.Library, onSelect = {}, accountName = "Leonard", onAccount = {}, modifier = Modifier.focusOnStart())
            }
        }
    }

    @Composable
    private fun Buttons(focusFirst: Boolean, focusSecondary: Boolean = false) {
        Page {
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s6)) {
                ActionButton(
                    "Resume at 32:10",
                    {},
                    icon = Icons.Rounded.PlayArrow,
                    modifier = if (focusFirst) Modifier.focusOnStart() else Modifier,
                )
                ActionButton(
                    "All episodes",
                    {},
                    style = ActionStyle.Secondary,
                    modifier = if (focusSecondary) Modifier.focusOnStart() else Modifier,
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s6)) {
                ActionButton("Download and play episode 1", {}, icon = Icons.Rounded.PlayArrow, size = ActionSize.Large)
                ActionButton("Follow the series", {}, style = ActionStyle.Secondary, size = ActionSize.Large)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s6)) {
                ActionButton("Download", {}, busy = true, busyText = "Starting…")
                ActionButton("Delete", {}, style = ActionStyle.Secondary, enabled = false)
                ActionButton("Read all", {}, style = ActionStyle.Secondary, size = ActionSize.Small)
            }
            Row(
                Modifier.background(IrisColor.stageRaised).padding(IrisSpace.s5),
                horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconAction(Icons.Rounded.Pause, "Pause", {})
                ActionButton("Audio and subtitles", {}, icon = Icons.Rounded.Subtitles, style = ActionStyle.Quiet)
                IconAction(Icons.Rounded.Search, "Search, something new", {}, badge = true)
            }
        }
    }

    @Test
    fun buttons() {
        shots.snap("buttons") { Buttons(focusFirst = false) }
        shots.snap("buttons_primary_focused") { Buttons(focusFirst = true) }
        shots.snap("buttons_secondary_focused") { Buttons(focusFirst = false, focusSecondary = true) }
    }

    @Composable
    private fun Pills(focus: Boolean) {
        Page {
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4), verticalAlignment = Alignment.CenterVertically) {
                Text("View", style = IrisType.meta, color = IrisColor.inkMuted)
                PillChoice(listOf("Titles", "Grid", "List"), selected = "Titles", onSelect = {}, label = { it })
            }
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                Pill("Unwatched", selected = false, onClick = {}, modifier = if (focus) Modifier.focusOnStart() else Modifier)
                Pill("New episodes", selected = true, onClick = {})
            }
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                Chip("Freeleech", tone = ChipTone.Ok)
                Chip("V3X")
                Chip("English audio, French subtitles (VOSTFR)")
                Chip("Few seeders", tone = ChipTone.Warn)
                Chip("Dead torrent", tone = ChipTone.Down)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                Chip("1080p", size = ChipSize.Small)
                LanguageChip("french")
                LanguageChip("multi")
                LanguageChip("unknown")
            }
        }
    }

    @Test
    fun pills() {
        shots.snap("pills") { Pills(focus = false) }
        shots.snap("pills_focused") { Pills(focus = true) }
    }

    @Composable
    private fun Cards(focusPoster: Boolean, focusStill: Boolean) {
        Column(Modifier.padding(top = IrisLayout.current.safeVertical), verticalArrangement = Arrangement.spacedBy(IrisSpace.s7)) {
            CardRow(items = (0..3).toList(), key = { it }) { i ->
                StillCard(
                    title = listOf("Severance", "Shōgun", "Perfect Days", "Frieren")[i],
                    imageUrl = null,
                    onClick = {},
                    progress = listOf(0.58f, 0.22f, 0.42f, 0.62f)[i],
                    meta = listOf("S2:E4 · 23 min left", "S1:E7 · 41 min left", "Movie · 1 h 12 min left", "E19 · 9 min left")[i],
                    modifier = if (focusStill && i == 0) Modifier.focusOnStart() else Modifier,
                )
            }
            CardRow(items = (0..5).toList(), key = { it }) { i ->
                PosterCard(
                    title = listOf("Severance", "Dune: Part Two", "The Bear", "Arcane", "Severed Ties", "Andor")[i],
                    imageUrl = null,
                    onClick = {},
                    kind = listOf("Series", "Movie", "Series", "Series", "Movie · 1992", "Series")[i],
                    badge = if (i == 4) "C411" else null,
                    progress = if (i == 0) 0.58f else null,
                    status = listOf("All 19 episodes on disk", "Unwatched", "Downloading S4 · 42 %", "9 of 18 on disk", "No release found", "Watched")[i],
                    statusTone = listOf(StatusTone.Ok, StatusTone.Muted, StatusTone.Ok, StatusTone.Warn, StatusTone.Down, StatusTone.Muted)[i],
                    modifier = if (focusPoster && i == 1) Modifier.focusOnStart() else Modifier,
                )
            }
        }
    }

    @Test
    fun cards() {
        shots.snap("cards") { Cards(focusPoster = false, focusStill = false) }
        shots.snap("cards_poster_focused") { Cards(focusPoster = true, focusStill = false) }
        shots.snap("cards_still_focused") { Cards(focusPoster = false, focusStill = true) }
    }

    @Composable
    private fun Rows(focus: Boolean) {
        Page {
            listOf("Season 2 complete, 1080p", "Episode 4, 2160p HDR").forEachIndexed { i, what ->
                RowCard(onClick = {}, modifier = if (focus && i == 0) Modifier.focusOnStart() else Modifier) {
                    Artwork("Severance", null, width = IrisSize.posterMini, showTitle = false)
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
                        Text(what, style = IrisType.bodyStrong.copy(fontSize = IrisType.action.fontSize))
                        Text("Severance.S02.VOSTFR.1080p.WEB.H264", style = IrisType.mono, color = IrisColor.inkMuted)
                        StatusLine("61 seeders · 14.8 GB · 1 week ago", tone = StatusTone.Ok)
                    }
                    Chip("V3X", size = ChipSize.Small)
                }
            }
            FramedBlock(Modifier.fillMaxWidth()) {
                SectionTitle("Release notes from V3X", meta = "Written by the uploader", style = IrisType.group)
                Text("Severance, season 2, complete. 10 episodes in 1080p.", style = IrisType.reading)
            }
        }
    }

    @Test
    fun rows() {
        shots.snap("rows") { Rows(focus = false) }
        shots.snap("rows_focused") { Rows(focus = true) }
    }

    @Test
    fun labels() = shots.snap("labels") {
        Page {
            SectionTitle("Releases on the trackers", meta = "24 found · 3 trackers answered, C411 did not")
            Column(Modifier.width(320.dp), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                Meter(0.58f)
                StatusLine("In your library", tone = StatusTone.Ok)
                StatusLine("4 releases")
                StatusLine("9 of 18 on disk", tone = StatusTone.Warn)
                StatusLine("No release found", tone = StatusTone.Down)
                StaleNotice(UiError("Can't reach the Iris server."))
            }
            Column(Modifier.width(320.dp)) {
                FactRow("Swarm", "61 seeders · 9 leechers · 14.8 GB")
                FactRow("Video", "1080p · H.264 · 23.976 fps")
            }
            KeyHints(
                listOf(KeyHint(Keys.OK, "Play"), KeyHint(Keys.HOLD_OK, "Remove, mark watched"), KeyHint(Keys.BACK, "To the menu")),
                trailing = "Row 1 of 10",
            )
        }
    }

    @Test
    fun steps() = shots.snap("steps") {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Column(Modifier.width(560.dp), verticalArrangement = Arrangement.spacedBy(IrisSpace.s5)) {
                Text("Getting ready", style = IrisType.meta, color = IrisColor.inkMuted)
                Text("Severance · S2:E1", style = IrisType.headline)
                StepList(
                    listOf(
                        Step("Connected to peers", StepState.Done, "38 peers"),
                        Step("Read the video details", StepState.Done, "1080p H.264, plays directly"),
                        Step("Downloading the first minutes", StepState.Current, "64 % · 8.2 MB/s", progress = 0.64f),
                        Step("Start playback", StepState.Pending),
                    ),
                )
            }
        }
    }

    @Composable
    private fun Tracks(focusSelected: Boolean) {
        Box(Modifier.fillMaxSize().background(IrisColor.stageRaised)) {
            SidePanel(
                title = "Audio and subtitles",
                onDismiss = {},
                footer = "Kept for the whole series. Subtitle size and colors follow the Android caption settings.",
            ) {
                PanelLabel("Audio")
                PanelOptions(listOf("English, original", "French (VF)"), selected = "English, original", onSelect = {}, label = { it })
                PanelLabel("Subtitles")
                PanelOption("Off", selected = false, onClick = {})
                PanelOption(
                    "English, for deaf and hard of hearing (SDH)",
                    selected = true,
                    onClick = {},
                    modifier = if (focusSelected) Modifier.focusOnStart() else Modifier,
                )
                PanelOption("French", selected = false, onClick = {})
            }
        }
    }

    @Test
    fun sidePanel() {
        shots.snap("side_panel") { Tracks(focusSelected = false) }
        shots.snap("side_panel_focused") { Tracks(focusSelected = true) }
    }

    @Test
    fun states() = shots.snap("states") {
        Row(Modifier.fillMaxSize()) {
            Box(Modifier.weight(1f)) { LoadingState(label = "Loading your library…") }
            Box(Modifier.weight(1f)) { EmptyState("No release found", body = "No tracker has this title yet.", actionLabel = "Search again", onAction = {}) }
            Box(Modifier.weight(1f)) { ErrorState("Can't reach the Iris server. Check the connection, then try again.", onRetry = {}) }
        }
    }

    @Test
    fun textInput() {
        shots.snap("text_input") {
            Page {
                TextInput("", {}, label = "Title, year or release name", leadingIcon = Icons.Rounded.Search, modifier = Modifier.width(260.dp))
                TextInput("sever", {}, label = "Search", leadingIcon = Icons.Rounded.Search, modifier = Modifier.width(260.dp))
            }
        }
        shots.snap("text_input_focused") {
            Page {
                TextInput("sever", {}, label = "Search", leadingIcon = Icons.Rounded.Search, modifier = Modifier.width(260.dp).focusOnStart())
            }
        }
    }

    @Test
    fun confirmDialog() = shots.snap("confirm_dialog") {
        ConfirmDialog(
            eyebrow = "Delete",
            title = "Delete Severance, season 2?",
            body = "The files leave the disk. Your watch progress stays.",
            confirmLabel = "Delete",
            onConfirm = {},
            onCancel = {},
        )
    }

    @Test
    fun posterGrid() = shots.snapEverySize("poster_grid") {
        PosterGrid(items = (1..14).toList(), key = { it }, minCell = IrisSize.posterGridMin) { i ->
            PosterCard(title = "Title $i", imageUrl = null, onClick = {}, width = null, kind = "Movie", status = "Unwatched")
        }
    }
}
