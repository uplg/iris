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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridScope
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Public
import androidx.compose.material.icons.rounded.Search
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.ColorMatrix
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
import kotlinx.coroutines.flow.first
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
import studio.kahn.iris.tv.ui.components.FocusReturn
import studio.kahn.iris.tv.ui.components.FocusSurface
import studio.kahn.iris.tv.ui.components.LoadingState
import studio.kahn.iris.tv.ui.components.Meter
import studio.kahn.iris.tv.ui.components.PanelLabel
import studio.kahn.iris.tv.ui.components.PanelOptions
import studio.kahn.iris.tv.ui.components.Pill
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.SidePanel
import studio.kahn.iris.tv.ui.components.TextInput
import studio.kahn.iris.tv.ui.components.focusReturn
import studio.kahn.iris.tv.ui.components.rememberFocusReturn
import studio.kahn.iris.tv.ui.format.plural
import studio.kahn.iris.tv.ui.format.timeLeft
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
    /** The household's usual countries, first in the picker. */
    val usual: List<LiveCountry> = emptyList(),
)

/** A channel line, wherever it comes from (a country's list, a search hit). */
@Immutable
data class ChannelTileUi(
    val country: String,
    val channel: LiveChannel,
    /** The country's name, on search hits (they span every country). */
    val countryLabel: String? = null,
)

private const val ALL = "all"

/**
 * Live TV (web `/live`), as a guide: the page title, a channel search across
 * every country, the country (a side panel to change it: the usual ones first,
 * then every country by name, each with its channel count, a field to narrow
 * it), then the country's channels: a category filter, the numbered TNT ones
 * first, then by category, each with what is on now (how far, how long left)
 * and next. A channel that will likely not play says why and steps back.
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
    initiallyPicking: Boolean = false,
) {
    val layout = IrisLayout.current
    var picking by remember { mutableStateOf(initiallyPicking) }
    val cards = rememberFocusReturn()
    val focus = remember(cards) { GridFocus(cards) }
    val current = ui.countries.firstOrNull { it.code == ui.country }
    Box(modifier.fillMaxSize().background(IrisColor.ground)) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
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
                    label = "Search every country",
                    leadingIcon = Icons.Rounded.Search,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { keyboard?.hide() }),
                    modifier = Modifier.width(minOf(260.dp, layout.width * 0.32f)),
                )
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
                    SearchResults(results, countryName, focus, onOpen, onRetry)
                } else {
                    CountryChannels(ui, current?.name ?: ui.country?.uppercase().orEmpty(), clock, focus, onOpen, onRetry)
                }
            }
        }
        if (picking) {
            CountryPanel(
                countries = ui.countries,
                usual = ui.usual,
                selected = current,
                onDismiss = { picking = false },
                onPick = {
                    picking = false
                    focus.placing = true
                    onPickCountry(it.code)
                },
            )
        }
    }
}

/**
 * The country picker: the household's usual countries, then every other one
 * by name, each with how many channels it carries; typing narrows the list
 * (the remote's keyboard, or a phone's). Opens on the country shown.
 */
@Composable
private fun CountryPanel(
    countries: List<LiveCountry>,
    usual: List<LiveCountry>,
    selected: LiveCountry?,
    onDismiss: () -> Unit,
    onPick: (LiveCountry) -> Unit,
) {
    var typed by remember { mutableStateOf("") }
    val byName = remember(countries) { countries.sortedBy { it.name } }
    SidePanel(title = "Country", onDismiss = onDismiss) {
        TextInput(
            value = typed,
            onValueChange = { typed = it },
            label = "Type a country",
            leadingIcon = Icons.Rounded.Search,
            modifier = Modifier.fillMaxWidth().padding(bottom = IrisSpace.s2),
        )
        if (typed.isNotBlank()) {
            val matches = findCountries(byName, typed)
            if (matches.isEmpty()) {
                Text("No country matches “${typed.trim()}”.", style = IrisType.meta, color = IrisColor.inkMuted, modifier = Modifier.padding(10.dp))
            }
            PanelOptions(options = matches, selected = selected, onSelect = onPick, label = ::countryLabel)
        } else {
            val first = usual.map { it.code }.toSet()
            if (usual.isNotEmpty()) {
                PanelLabel("Your countries")
                PanelOptions(options = usual, selected = selected, onSelect = onPick, label = ::countryLabel, focusOnOpen = true)
            }
            PanelLabel("All countries")
            PanelOptions(
                options = byName.filter { it.code !in first },
                selected = selected,
                onSelect = onPick,
                label = ::countryLabel,
                focusOnOpen = usual.isEmpty(),
            )
        }
    }
}

@Composable
private fun CountryChannels(
    ui: LiveTvUi,
    name: String,
    clock: (OffsetDateTime) -> String,
    focus: GridFocus,
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
                EmptyState(
                    "No channels for this country yet",
                    body = "Pick another country, or search every country by a channel’s name.",
                )
            } else {
                var category by rememberSaveable(country) { mutableStateOf(ALL) }
                val sections = remember(channels) { channelSections(channels) }
                val filtered = if (sections.any { it.key == category }) category else ALL
                val shown = remember(sections, filtered, country) {
                    sections.filter { filtered == ALL || it.key == filtered }
                        .map { s -> s.title to s.channels.map { ChannelTileUi(country, it) } }
                }
                val guided = ui.guide.entries.isNotEmpty()
                ChannelGrid(
                    sections = shown,
                    guide = ui.guide,
                    clock = clock,
                    focus = focus,
                    onOpen = onOpen,
                    compact = !guided,
                    head = buildList {
                        add(GridHead("title") { SectionTitle("Channels in $name", meta = plural(channels.size, "channel")) })
                        if (sections.size > 1) {
                            add(GridHead("filter") { CategoryFilter(sections, channels.size, filtered) { category = it } })
                        }
                        if (!guided && ui.guide.readAtMs > 0L) {
                            add(
                                GridHead("noguide") {
                                    Text(
                                        "No programme guide for this country: channels show without what is on.",
                                        style = IrisType.meta,
                                        color = IrisColor.inkMuted,
                                    )
                                },
                            )
                        }
                    },
                )
            }
        }
    }
}

/** One category at a time (web: the "Show" pills): a radio group that scrolls sideways. */
@Composable
private fun CategoryFilter(sections: List<ChannelSection>, total: Int, selected: String, onSelect: (String) -> Unit) {
    val options = listOf(Triple(ALL, "All", total)) + sections.map { Triple(it.key, it.title, it.channels.size) }
    LazyRow(
        Modifier
            .fillMaxWidth()
            .selectableGroup()
            .focusRestorer()
            .focusGroup(),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2),
        contentPadding = PaddingValues(vertical = IrisSpace.s1),
    ) {
        items(options, key = { it.first }) { (key, title, count) ->
            Pill(
                text = "$title  $count",
                selected = key == selected,
                onClick = { onSelect(key) },
                role = Role.RadioButton,
            )
        }
    }
}

@Composable
private fun SearchResults(
    results: Loadable<LiveResults>,
    countryName: (String) -> String,
    focus: GridFocus,
    onOpen: (String, String) -> Unit,
    onRetry: () -> Unit,
) {
    when (results) {
        Loadable.Loading -> LoadingState(label = "Searching every country…")
        is Loadable.Failed -> ErrorState(results.error.message, onRetry, title = "The search failed")
        is Loadable.Ready, is Loadable.Stale -> {
            val r = results.valueOrNull ?: return
            if (r.byCountry.isEmpty()) {
                EmptyState("No channel matches “${r.query}”, in any country")
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
                            )
                        }
                    }
                }
                val total = r.byCountry.sumOf { it.second.size }
                ChannelGrid(
                    sections = sections,
                    guide = LiveGuide(),
                    clock = { "" },
                    focus = focus,
                    onOpen = onOpen,
                    compact = true,
                    head = listOf(
                        GridHead("title") {
                            SectionTitle(
                                "Channels matching “${r.query}” in every country",
                                meta = "${plural(total, "channel")} in ${plural(r.byCountry.size, "country", "countries")}",
                            )
                        },
                    ),
                )
            }
        }
    }
}

/** A full-width line above the channels (the title, the filter, a note). */
private class GridHead(val key: String, val content: @Composable () -> Unit)

/**
 * Where the grid's focus goes: [cards] knows the channel left (kept across opening it and
 * coming back); [placing] asks the grid to place the focus, on entering the screen (back from
 * a channel too) and after a country is picked, never while the search field is typed in.
 */
@Stable
private class GridFocus(val cards: FocusReturn) {
    var placing by mutableStateOf(true)
}

private fun tileKey(t: ChannelTileUi) = "${t.country}:${t.channel.id}"

@Composable
private fun ChannelGrid(
    sections: List<Pair<String, List<ChannelTileUi>>>,
    guide: LiveGuide,
    clock: (OffsetDateTime) -> String,
    focus: GridFocus,
    onOpen: (String, String) -> Unit,
    compact: Boolean,
    head: List<GridHead>,
) {
    val layout = IrisLayout.current
    val grid = rememberLazyGridState()
    // Each tile's item index: the heads, then per section its heading and its tiles.
    val indexOf = remember(sections, head.size) {
        var at = head.size
        buildMap {
            sections.forEach { (_, tiles) ->
                at++
                tiles.forEach { putIfAbsent(tileKey(it), at++) }
            }
        }
    }
    val firstKey = sections.firstOrNull()?.second?.firstOrNull()?.let(::tileKey)
    // The channel left when it is still listed, else the first one; once it is placed (a bare
    // request can run before the lazy item exists).
    LaunchedEffect(focus.placing, indexOf) {
        if (!focus.placing) return@LaunchedEffect
        val key = focus.cards.last?.takeIf { it in indexOf } ?: firstKey ?: return@LaunchedEffect
        val index = indexOf.getValue(key)
        if (grid.layoutInfo.visibleItemsInfo.none { it.index == index }) grid.scrollToItem(index)
        snapshotFlow { grid.layoutInfo.visibleItemsInfo.any { it.index == index } }.first { it }
        withFrameNanos { }
        if (focus.cards.focus(listOf(key))) focus.placing = false
    }
    LazyVerticalGrid(
        columns = GridCells.Adaptive(if (compact) 200.dp else 270.dp),
        state = grid,
        modifier = Modifier
            .fillMaxSize()
            // Not while placing: the restorer would send that request to its own fallback.
            .then(if (!focus.placing && firstKey != null) Modifier.focusRestorer(focus.cards.requester(firstKey)) else Modifier)
            .focusGroup(),
        contentPadding = PaddingValues(
            start = layout.safeHorizontal,
            end = layout.safeHorizontal,
            top = IrisSpace.s1,
            bottom = layout.safeVertical,
        ),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
    ) {
        head.forEach { h -> item(key = h.key, span = { GridItemSpan(maxLineSpan) }) { h.content() } }
        sections.forEach { (title, tiles) ->
            sectionHeader(title, tiles.size)
            items(tiles, key = { t -> "${t.country}:$title:${t.channel.id}" }) { tile ->
                ChannelCard(
                    tile = tile,
                    nowNext = guide.entries[tile.channel.id],
                    nowMs = guide.readAtMs,
                    clock = clock,
                    guided = guide.entries.isNotEmpty(),
                    onClick = { onOpen(tile.country, tile.channel.id) },
                    modifier = Modifier.focusReturn(focus.cards, tileKey(tile)),
                )
            }
        }
    }
}

private fun LazyGridScope.sectionHeader(title: String, count: Int) {
    item(key = "head:$title", span = { GridItemSpan(maxLineSpan) }) {
        SectionTitle(title, meta = count.toString(), style = IrisType.group, modifier = Modifier.padding(top = IrisSpace.s3))
    }
}

/**
 * A channel as a guide line: its logo on a plate that suits it, the TNT
 * number, the name, what is on now (how far, how long left) and next; or why
 * it may not play, in words, the line set back.
 */
@Composable
fun ChannelCard(
    tile: ChannelTileUi,
    nowNext: LiveNowNext?,
    nowMs: Long,
    clock: (OffsetDateTime) -> String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    guided: Boolean = false,
) {
    val channel = tile.channel
    val dim = dimmed(channel)
    val notice = channelNotice(channel)
    FocusSurface(
        onClick = onClick,
        modifier = modifier.fillMaxWidth(),
        shape = IrisShape.card,
        colors = if (dim) FocusColors.Row.copy(container = IrisColor.ground) else FocusColors.Row,
        contentAlignment = Alignment.CenterStart,
    ) { _ ->
        Row(
            Modifier.fillMaxWidth().padding(IrisSpace.s3),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            LogoWell(channel, dim)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
                Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2), verticalAlignment = Alignment.CenterVertically) {
                    channel.tntNumber?.let { Chip(it.toString(), size = ChipSize.Small) }
                    Text(
                        channel.name,
                        style = IrisType.bodyStrong,
                        color = if (dim) IrisColor.inkMuted else IrisColor.ink,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                val now = nowNext?.now
                val next = nowNext?.next
                if (now != null) {
                    Text(
                        now.title,
                        style = IrisType.body,
                        color = IrisColor.ink,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.semantics { contentDescription = "Now: ${now.title}" },
                    )
                    val progress = programmeProgress(now.start, now.stop, nowMs)
                    val leftS = (now.stop.toInstant().toEpochMilli() - nowMs).coerceAtLeast(0L) / 1000.0
                    val until = clock(now.stop)
                    // the next programme's time already says when this one ends
                    val left = when {
                        progress == null -> if (until.isNotEmpty()) "Until $until" else ""
                        next != null -> timeLeft(leftS)
                        until.isNotEmpty() -> "Until $until, ${timeLeft(leftS)}"
                        else -> timeLeft(leftS)
                    }
                    if (left.isNotEmpty()) Text(left, style = IrisType.metaSmall, color = IrisColor.inkMuted, maxLines = 1)
                    if (progress != null) Meter(progress, height = 2.dp, track = IrisColor.line)
                    if (next != null) {
                        Text(nextWords(next, clock), style = IrisType.metaSmall, color = IrisColor.inkMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                } else if (guided && notice == null) {
                    Text("No guide for this channel", style = IrisType.metaSmall, color = IrisColor.inkMuted, maxLines = 1)
                }
                if (notice != null) Text(notice, style = IrisType.metaSmall, color = IrisColor.inkMuted, maxLines = 1)
            }
        }
    }
}

private val grey = ColorFilter.colorMatrix(ColorMatrix().apply { setToSaturation(0f) })

@Composable
private fun LogoWell(channel: LiveChannel, dim: Boolean) {
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
            .size(width = 72.dp, height = 44.dp)
            .alpha(if (dim) 0.55f else 1f)
            .clip(IrisShape.key)
            .background(if (logo != null) tone.plate else IrisColor.art),
        contentAlignment = Alignment.Center,
    ) {
        if (logo != null) {
            AsyncImage(
                // Software bitmaps only while the luminance pass still has to read the pixels:
                // a tone already known keeps the logo in a hardware bitmap (2 GB boxes).
                model = ImageRequest.Builder(LocalPlatformContext.current).data(logo)
                    .allowHardware(logoToneCache.containsKey(logo)).build(),
                contentDescription = null,
                contentScale = ContentScale.Fit,
                colorFilter = if (dim) grey else null,
                modifier = Modifier.fillMaxSize().padding(IrisSpace.s1),
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
    }
}
