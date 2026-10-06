package studio.kahn.iris.tv.ui.screens.live

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridScope
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Public
import androidx.compose.material.icons.rounded.Search
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import coil3.compose.LocalPlatformContext
import coil3.request.ImageRequest
import coil3.request.allowHardware
import coil3.toBitmap
import java.time.OffsetDateTime
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveCountry
import studio.kahn.iris.tv.data.LiveNowNext
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Chip
import studio.kahn.iris.tv.ui.components.ChipSize
import studio.kahn.iris.tv.ui.components.EmptyState
import studio.kahn.iris.tv.ui.components.ErrorState
import studio.kahn.iris.tv.ui.components.FocusColors
import studio.kahn.iris.tv.ui.components.FocusSurface
import studio.kahn.iris.tv.ui.components.LoadingState
import studio.kahn.iris.tv.ui.components.Meter
import studio.kahn.iris.tv.ui.components.PanelOptions
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.SidePanel
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.TextInput
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** What the channel list draws. */
@Immutable
data class LiveTvUi(
    val countries: List<LiveCountry>,
    val country: String?,
    val channels: Loadable<List<LiveChannel>>,
    val guide: LiveGuide,
    val query: String,
    val results: Loadable<LiveResults>?,
)

/** A channel tile, wherever it comes from (a country's list, a search hit). */
@Immutable
data class ChannelTileUi(
    val country: String,
    val channel: LiveChannel,
    /** The country's name, on search hits (they span every country). */
    val countryLabel: String? = null,
)

/**
 * Live TV (web `/live`): the page title, a channel search across every
 * country, the country (a side panel to change it), then the country's
 * channels, the numbered TNT ones first, then by category, each with what is
 * on now and how far into it. Opening a channel plays it.
 */
@Composable
fun LiveTvContent(
    ui: LiveTvUi,
    clock: (OffsetDateTime) -> String,
    countryName: (String) -> String,
    onQueryChange: (String) -> Unit,
    onPickCountry: (String) -> Unit,
    onOpen: (country: String, channelId: String) -> Unit,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    focusedOnce: MutableState<Boolean> = remember { mutableStateOf(false) },
) {
    val layout = IrisLayout.current
    var picking by remember { mutableStateOf(false) }
    Box(modifier.fillMaxSize().background(IrisColor.ground)) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(IrisSpace.s5)) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = layout.safeHorizontal),
                horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                SectionTitle("Live TV", style = IrisType.page)
                Spacer(Modifier.weight(1f))
                val keyboard = LocalSoftwareKeyboardController.current
                TextInput(
                    value = ui.query,
                    onValueChange = onQueryChange,
                    label = "Search channels, every country",
                    leadingIcon = Icons.Rounded.Search,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { keyboard?.hide() }),
                    modifier = Modifier.width(minOf(260.dp, layout.width * 0.32f)),
                )
                val current = ui.countries.firstOrNull { it.code == ui.country }
                ActionButton(
                    text = current?.let { "${it.flag} ${it.name}" } ?: ui.country?.uppercase() ?: "Country",
                    onClick = { picking = true },
                    style = ActionStyle.Secondary,
                    size = ActionSize.Large,
                    icon = Icons.Rounded.Public,
                    enabled = ui.countries.isNotEmpty(),
                )
            }
            val results = ui.results
            Box(Modifier.weight(1f)) {
                if (results != null) {
                    SearchResults(results, countryName, focusedOnce, onOpen, onRetry)
                } else {
                    CountryChannels(ui, clock, focusedOnce, onOpen, onRetry)
                }
            }
        }
        if (picking) {
            SidePanel(title = "Country", onDismiss = { picking = false }) {
                PanelOptions(
                    options = ui.countries,
                    selected = ui.countries.firstOrNull { it.code == ui.country },
                    onSelect = {
                        picking = false
                        onPickCountry(it.code)
                    },
                    label = { "${it.flag} ${it.name}" },
                    focusOnOpen = true,
                )
            }
        }
    }
}

@Composable
private fun CountryChannels(
    ui: LiveTvUi,
    clock: (OffsetDateTime) -> String,
    focusedOnce: MutableState<Boolean>,
    onOpen: (String, String) -> Unit,
    onRetry: () -> Unit,
) {
    when (val state = ui.channels) {
        Loadable.Loading -> LoadingState(label = "Loading channels…")
        is Loadable.Failed -> ErrorState(state.error.message, onRetry, title = "Couldn't load the channels")
        is Loadable.Ready, is Loadable.Stale -> {
            val channels = state.valueOrNull.orEmpty()
            val country = ui.country.orEmpty()
            if (channels.isEmpty()) {
                EmptyState("No channels for this country", body = "Pick another country, or search every country by name.")
            } else {
                val sections = remember(channels) {
                    channelSections(channels).map { s -> s.title to s.channels.map { ChannelTileUi(country, it) } }
                }
                ChannelGrid(sections, ui.guide, clock, focusedOnce, onOpen)
            }
        }
    }
}

@Composable
private fun SearchResults(
    results: Loadable<LiveResults>,
    countryName: (String) -> String,
    focusedOnce: MutableState<Boolean>,
    onOpen: (String, String) -> Unit,
    onRetry: () -> Unit,
) {
    when (results) {
        Loadable.Loading -> LoadingState(label = "Searching…")
        is Loadable.Failed -> ErrorState(results.error.message, onRetry, title = "The search failed")
        is Loadable.Ready, is Loadable.Stale -> {
            val r = results.valueOrNull ?: return
            if (r.byCountry.isEmpty()) {
                EmptyState("No channel matches “${r.query}”")
            } else {
                val sections = remember(r) {
                    r.byCountry.map { (code, hits) ->
                        countryName(code) to hits.map { hit ->
                            ChannelTileUi(
                                country = code,
                                channel = LiveChannel(
                                    categories = emptyList(),
                                    geoBlocked = false,
                                    id = hit.id,
                                    name = hit.name,
                                    not247 = false,
                                    logoOrigin = hit.logoOrigin,
                                    logoUrl = hit.logoUrl,
                                ),
                                countryLabel = countryName(code),
                            )
                        }
                    }
                }
                ChannelGrid(sections, LiveGuide(), { "" }, focusedOnce, onOpen)
            }
        }
    }
}

@Composable
private fun ChannelGrid(
    sections: List<Pair<String, List<ChannelTileUi>>>,
    guide: LiveGuide,
    clock: (OffsetDateTime) -> String,
    focusedOnce: MutableState<Boolean>,
    onOpen: (String, String) -> Unit,
) {
    val layout = IrisLayout.current
    // The first channel takes the focus once, after it is placed (a bare
    // effect can run before the lazy item exists); a remount for search
    // results must not steal it from the search field.
    val first = remember { FocusRequester() }
    LazyVerticalGrid(
        columns = GridCells.Adaptive(150.dp),
        modifier = Modifier
            .fillMaxSize()
            .focusRestorer(first)
            .focusGroup(),
        contentPadding = PaddingValues(
            start = layout.safeHorizontal,
            end = layout.safeHorizontal,
            top = IrisSpace.s1,
            bottom = layout.safeVertical,
        ),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
        verticalArrangement = Arrangement.spacedBy(IrisSpace.s5),
    ) {
        sections.forEachIndexed { si, (title, tiles) ->
            sectionHeader(title, tiles.size, first = si == 0)
            itemsIndexed(tiles, key = { _, t -> "${t.country}:$title:${t.channel.id}" }) { i, tile ->
                val isFirst = si == 0 && i == 0
                ChannelCard(
                    tile = tile,
                    nowNext = guide.entries[tile.channel.id],
                    nowMs = guide.readAtMs,
                    clock = clock,
                    onClick = { onOpen(tile.country, tile.channel.id) },
                    modifier = if (isFirst) {
                        Modifier
                            .focusRequester(first)
                            .onGloballyPositioned {
                                if (!focusedOnce.value) {
                                    focusedOnce.value = true
                                    runCatching { first.requestFocus() }
                                }
                            }
                    } else {
                        Modifier
                    },
                )
            }
        }
    }
}

private fun LazyGridScope.sectionHeader(title: String, count: Int, first: Boolean) {
    item(key = "head:$title", span = { GridItemSpan(maxLineSpan) }) {
        SectionTitle(
            title,
            meta = if (count == 1) "1 channel" else "$count channels",
            modifier = Modifier.padding(top = if (first) 0.dp else IrisSpace.s5),
        )
    }
}

/**
 * A channel: its logo on a plate that suits it (a dark logo on a light plate
 * and the reverse), the TNT number, the name, what is on now with how far
 * into it, or why it may not play.
 */
@Composable
fun ChannelCard(
    tile: ChannelTileUi,
    nowNext: LiveNowNext?,
    nowMs: Long,
    clock: (OffsetDateTime) -> String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val channel = tile.channel
    FocusSurface(
        onClick = onClick,
        modifier = modifier.fillMaxWidth(),
        shape = IrisShape.card,
        colors = FocusColors.Row,
        contentAlignment = Alignment.TopStart,
    ) { _ ->
        Column(Modifier.fillMaxWidth().padding(IrisSpace.s3), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
            LogoWell(channel)
            Text(channel.name, style = IrisType.bodyStrong, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val now = nowNext?.now
            when {
                now != null -> {
                    Text(nowWords(now, clock), style = IrisType.metaSmall, color = IrisColor.inkMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    val progress = programmeProgress(now.start, now.stop, nowMs)
                    if (progress != null) Meter(progress, height = 2.dp, track = IrisColor.line)
                }
                tile.countryLabel != null -> Text(tile.countryLabel, style = IrisType.metaSmall, color = IrisColor.inkMuted, maxLines = 1)
                channel.geoBlocked -> StatusLine("May be blocked in your country", tone = StatusTone.Warn, style = IrisType.metaSmall)
                channel.not247 -> Text("Not on all day", style = IrisType.metaSmall, color = IrisColor.inkMuted, maxLines = 1)
                else -> Text(" ", style = IrisType.metaSmall)
            }
        }
    }
}

@Composable
private fun LogoWell(channel: LiveChannel) {
    // The signed proxy first, then the logo's origin (logo hosts rate-limit the
    // server's datacenter IP while this device's is fine), then the initial.
    val candidates = remember(channel.logoUrl, channel.logoOrigin) {
        listOfNotNull(channel.logoUrl, channel.logoOrigin).distinct()
    }
    var index by remember(candidates) { mutableIntStateOf(0) }
    val logo = candidates.getOrNull(index)
    var tone by remember(logo) { mutableStateOf(logo?.let { logoToneCache[it] } ?: LogoTone.Neutral) }
    Box(
        Modifier
            .fillMaxWidth()
            .height(44.dp)
            .clip(IrisShape.key)
            .background(if (logo != null) tone.plate else IrisColor.art),
        contentAlignment = Alignment.Center,
    ) {
        if (logo != null) {
            AsyncImage(
                // Software bitmaps: the luminance pass reads pixels.
                model = ImageRequest.Builder(LocalPlatformContext.current).data(logo).allowHardware(false).build(),
                contentDescription = null,
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxSize().padding(IrisSpace.s2),
                onError = { index++ },
                onSuccess = { state ->
                    tone = logoToneCache.getOrPut(logo) {
                        runCatching { logoTone(state.result.image.toBitmap()) }.getOrDefault(LogoTone.Neutral)
                    }
                },
            )
        } else {
            Text(channel.name.take(1).uppercase(), style = IrisType.group, color = IrisColor.artInk)
        }
        val number = channel.tntNumber
        if (number != null) {
            Chip(number.toString(), Modifier.align(Alignment.TopStart).padding(IrisSpace.s1), size = ChipSize.Small)
        }
    }
}
