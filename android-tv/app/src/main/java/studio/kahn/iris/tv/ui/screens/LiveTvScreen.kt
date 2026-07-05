package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Border
import androidx.tv.material3.ClickableSurfaceDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import coil3.compose.LocalPlatformContext
import coil3.request.ImageRequest
import coil3.request.allowHardware
import coil3.toBitmap
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveCountry
import studio.kahn.iris.tv.data.LiveNowNext
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.FontMono
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing

/** Refresh cadence for the now/next strip on the grid. */
private const val EPG_REFRESH_MS = 60_000L

/**
 * Live TV channel grid — mirrors the web `/live` page: a country picker,
 * the pinned "TNT" section (fr only, Arcom order, numbered badges), then
 * every remaining channel grouped by category, each card carrying the
 * programme now on air with a progress bar.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun LiveTvScreen(
    container: AppContainer,
    onOpenChannel: (country: String, channelId: String) -> Unit,
    onBack: () -> Unit,
) {
    val layout = LocalTvLayout.current

    var serverUrl by remember { mutableStateOf<String?>(null) }
    var countries by remember { mutableStateOf<List<LiveCountry>>(emptyList()) }
    var country by rememberSaveable { mutableStateOf<String?>(null) }
    var channels by remember { mutableStateOf<List<LiveChannel>>(emptyList()) }
    var epg by remember { mutableStateOf<Map<String, LiveNowNext>>(emptyMap()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var pickingCountry by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        val url = container.sessionStore.serverUrl.first()
            ?: run { error = "Not signed in"; loading = false; return@LaunchedEffect }
        serverUrl = url
        runCatching { container.apiFor(url).liveTvCountries() }
            .onSuccess { res ->
                countries = res.countries
                if (country == null) country = res.defaultCountry
            }
            .onFailure { if (country == null) country = "fr" }
    }

    // (Re)load channels whenever the country changes.
    LaunchedEffect(country, serverUrl) {
        val c = country ?: return@LaunchedEffect
        val url = serverUrl ?: return@LaunchedEffect
        loading = true
        error = null
        channels = emptyList()
        epg = emptyMap()
        runCatching { container.apiFor(url).liveTvChannels(c) }
            // Logo URLs are server-relative (`/api/livetv/logo?…`) — Coil
            // needs them absolute against the Iris base URL.
            .onSuccess { res ->
                channels = res.channels.map { it.copy(logoUrl = absolutize(url, it.logoUrl)) }
            }
            .onFailure { error = it.message ?: "Failed to load channels" }
        loading = false
    }

    // Keep now/next fresh while the grid is on screen.
    LaunchedEffect(country, serverUrl) {
        val c = country ?: return@LaunchedEffect
        val url = serverUrl ?: return@LaunchedEffect
        while (true) {
            runCatching { container.apiFor(url).liveTvEpgNow(c) }
                .onSuccess { res -> epg = res.propertyEntries.associateBy { it.channelId } }
            delay(EPG_REFRESH_MS)
        }
    }

    val sections = remember(channels) {
        val tnt = channels.filter { it.tntNumber != null }
        val rest = channels.filter { it.tntNumber == null }
        val buckets = LinkedHashMap<String, MutableList<LiveChannel>>()
        for (ch in rest) {
            buckets.getOrPut(ch.categories.firstOrNull() ?: "Other") { mutableListOf() }.add(ch)
        }
        Pair(tnt, buckets.toList())
    }

    Box(Modifier.fillMaxSize().background(IrisColors.Background)) {
        Column(
            Modifier.fillMaxSize().padding(
                horizontal = layout.gutterHorizontal,
                vertical = layout.gutterVertical,
            ),
            verticalArrangement = Arrangement.spacedBy(Spacing.lg),
        ) {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.Bottom,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(Spacing.xs)) {
                    Eyebrow("Live")
                    SectionTitle("Live TV")
                }
                Row(horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
                    val current = countries.firstOrNull { it.code == country }
                    IrisButton(
                        text = current?.let { "${it.flag} ${it.name}" } ?: "Country",
                        onClick = { pickingCountry = !pickingCountry },
                        variant = IrisButtonVariant.Ghost,
                    )
                    IrisButton("← Back", onBack, variant = IrisButtonVariant.Ghost)
                }
            }

            when {
                pickingCountry -> CountryPicker(
                    countries = countries,
                    onPick = { code ->
                        pickingCountry = false
                        country = code
                    },
                )
                loading && channels.isEmpty() -> Text(
                    "Loading channels…",
                    style = MaterialTheme.typography.bodyMedium,
                    color = IrisColors.MutedForeground,
                )
                error != null -> Text(error!!, color = MaterialTheme.colorScheme.error)
                channels.isEmpty() -> Text(
                    "No channels available for this country.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = IrisColors.MutedForeground,
                )
                else -> ChannelGrid(
                    tnt = sections.first,
                    categories = sections.second,
                    epg = epg,
                    onOpen = { ch -> country?.let { onOpenChannel(it, ch.id) } },
                )
            }
        }
    }
}

@Composable
private fun ChannelGrid(
    tnt: List<LiveChannel>,
    categories: List<Pair<String, List<LiveChannel>>>,
    epg: Map<String, LiveNowNext>,
    onOpen: (LiveChannel) -> Unit,
) {
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = 200.dp),
        modifier = Modifier.fillMaxSize(),
        horizontalArrangement = Arrangement.spacedBy(Spacing.md),
        verticalArrangement = Arrangement.spacedBy(Spacing.md),
        contentPadding = PaddingValues(vertical = Spacing.sm),
    ) {
        if (tnt.isNotEmpty()) {
            item(key = "head-tnt", span = { GridItemSpan(maxLineSpan) }) {
                GridSectionHeader("TNT")
            }
            items(tnt, key = { "tnt-${it.id}" }) { ch ->
                ChannelCard(channel = ch, nowNext = epg[ch.id], onClick = { onOpen(ch) })
            }
        }
        for ((title, list) in categories) {
            item(key = "head-$title", span = { GridItemSpan(maxLineSpan) }) {
                GridSectionHeader(title)
            }
            items(list, key = { "$title-${it.id}" }) { ch ->
                ChannelCard(channel = ch, nowNext = epg[ch.id], onClick = { onOpen(ch) })
            }
        }
    }
}

@Composable
private fun GridSectionHeader(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.titleMedium,
        color = IrisColors.Foreground,
        modifier = Modifier.padding(top = Spacing.md, bottom = Spacing.xs),
    )
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ChannelCard(
    channel: LiveChannel,
    nowNext: LiveNowNext?,
    onClick: () -> Unit,
) {
    val shape = RoundedCornerShape(Radius.lg)
    Surface(
        onClick = onClick,
        shape = ClickableSurfaceDefaults.shape(shape = shape),
        scale = ClickableSurfaceDefaults.scale(focusedScale = Focus.controlScale),
        colors = ClickableSurfaceDefaults.colors(
            containerColor = IrisColors.Card,
            contentColor = IrisColors.Foreground,
            focusedContainerColor = IrisColors.Elev2,
            focusedContentColor = IrisColors.Foreground,
        ),
        border = ClickableSurfaceDefaults.border(
            border = Border.None,
            focusedBorder = Border(
                BorderStroke(Focus.ring, IrisColors.Brand),
                shape = shape,
            ),
        ),
    ) {
        Column(
            Modifier.fillMaxWidth().padding(Spacing.md),
            verticalArrangement = Arrangement.spacedBy(Spacing.sm),
        ) {
            // Adaptive "logo well": channel logos are wild PNGs — black ink
            // vanishes on a dark plate, white ink on a light one (the
            // classic black-on-black trap). The plate color is derived from
            // the decoded logo's own luminance: dark logo → light plate,
            // light logo → near-black plate, colorful/mid → neutral gray.
            val logo = channel.logoUrl
            var tone by remember(logo) {
                mutableStateOf(logo?.let { logoToneCache[it] } ?: LogoTone.Neutral)
            }
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(56.dp)
                    .background(tone.well(), RoundedCornerShape(Radius.md)),
            ) {
                if (logo != null) {
                    AsyncImage(
                        // Software bitmap so the pixels are readable for the
                        // luminance pass (hardware bitmaps throw on getPixel).
                        model = ImageRequest.Builder(LocalPlatformContext.current)
                            .data(logo)
                            .allowHardware(false)
                            .build(),
                        contentDescription = null,
                        modifier = Modifier.fillMaxSize().padding(Spacing.sm),
                        contentScale = ContentScale.Fit,
                        onSuccess = { state ->
                            tone = logoToneCache.getOrPut(logo) {
                                runCatching { logoTone(state.result.image.toBitmap()) }
                                    .getOrDefault(LogoTone.Neutral)
                            }
                        },
                    )
                } else {
                    Text(
                        channel.name.take(1).uppercase(),
                        style = MaterialTheme.typography.titleMedium,
                        color = IrisColors.Foreground,
                        modifier = Modifier.align(Alignment.Center),
                    )
                }
                val badge = channel.tntNumber?.toString()
                if (badge != null) {
                    Text(
                        badge,
                        style = MaterialTheme.typography.labelSmall.copy(fontFamily = FontMono),
                        color = Color.White,
                        modifier = Modifier
                            .align(Alignment.TopStart)
                            .padding(Spacing.xs)
                            .background(
                                Color.Black.copy(alpha = 0.65f),
                                RoundedCornerShape(Radius.sm),
                            )
                            .padding(horizontal = 5.dp, vertical = 1.dp),
                    )
                }
            }
            Text(
                channel.name,
                style = MaterialTheme.typography.titleSmall,
                color = IrisColors.Foreground,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            val now = nowNext?.now
            if (now != null) {
                Text(
                    now.title,
                    style = MaterialTheme.typography.bodySmall,
                    color = IrisColors.MutedForeground,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                ProgrammeProgressBar(
                    startEpochMs = now.start.toInstant().toEpochMilli(),
                    stopEpochMs = now.stop.toInstant().toEpochMilli(),
                )
            } else {
                Text(
                    when {
                        channel.geoBlocked -> "May be geo-blocked"
                        channel.not247 -> "Not 24/7"
                        else -> " "
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = IrisColors.FgDim,
                    maxLines = 1,
                )
            }
        }
    }
}

/** Resolve a server-relative path against the Iris base URL. */
private fun absolutize(base: String, path: String?): String? {
    if (path == null || !path.startsWith('/')) return path
    return base.trimEnd('/') + path
}

/** Well plate tones behind a channel logo, elected from its luminance. */
private enum class LogoTone {
    Light,
    Neutral,
    Dark,
    ;

    fun well(): Color = when (this) {
        Light -> Color.White.copy(alpha = 0.92f)
        Neutral -> Color(0xFF7A7D84).copy(alpha = 0.55f)
        Dark -> IrisColors.BackgroundDeep
    }
}

/** Per-URL memo — the grid re-composes on every EPG tick; one luminance
 *  pass per logo is plenty. */
private val logoToneCache = java.util.concurrent.ConcurrentHashMap<String, LogoTone>()

/** Mean luminance of the logo's opaque pixels → plate tone. Mirrors the web
 *  `logo-tone.ts` thresholds so both clients pick the same plate. */
private fun logoTone(bitmap: android.graphics.Bitmap): LogoTone {
    val stepX = maxOf(1, bitmap.width / 32)
    val stepY = maxOf(1, bitmap.height / 32)
    var luma = 0.0
    var count = 0
    var y = 0
    while (y < bitmap.height) {
        var x = 0
        while (x < bitmap.width) {
            val px = bitmap.getPixel(x, y)
            val alpha = px ushr 24 and 0xFF
            if (alpha > 25) {
                val r = px ushr 16 and 0xFF
                val g = px ushr 8 and 0xFF
                val b = px and 0xFF
                luma += 0.2126 * r + 0.7152 * g + 0.0722 * b
                count++
            }
            x += stepX
        }
        y += stepY
    }
    if (count == 0) return LogoTone.Neutral
    val mean = luma / count / 255.0
    return when {
        mean < 0.38 -> LogoTone.Light
        mean > 0.62 -> LogoTone.Dark
        else -> LogoTone.Neutral
    }
}

/** Thin red bar showing how far into the current programme we are. */
@Composable
fun ProgrammeProgressBar(startEpochMs: Long, stopEpochMs: Long) {
    val span = stopEpochMs - startEpochMs
    if (span <= 0) return
    val fraction = ((System.currentTimeMillis() - startEpochMs).toFloat() / span)
        .coerceIn(0f, 1f)
    Box(
        Modifier
            .fillMaxWidth()
            .height(3.dp)
            .background(IrisColors.Overlay12, RoundedCornerShape(Radius.pill)),
    ) {
        Box(
            Modifier
                .fillMaxWidth(fraction)
                .height(3.dp)
                .background(IrisColors.Destructive, RoundedCornerShape(Radius.pill)),
        )
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun CountryPicker(
    countries: List<LiveCountry>,
    onPick: (String) -> Unit,
) {
    if (countries.isEmpty()) {
        Text(
            "Country list unavailable.",
            style = MaterialTheme.typography.bodyMedium,
            color = IrisColors.MutedForeground,
        )
        return
    }
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = 220.dp),
        modifier = Modifier.fillMaxSize(),
        horizontalArrangement = Arrangement.spacedBy(Spacing.sm),
        verticalArrangement = Arrangement.spacedBy(Spacing.sm),
        contentPadding = PaddingValues(vertical = Spacing.sm),
    ) {
        items(countries, key = { it.code }) { c ->
            Surface(
                onClick = { onPick(c.code) },
                shape = ClickableSurfaceDefaults.shape(shape = RoundedCornerShape(Radius.md)),
                scale = ClickableSurfaceDefaults.scale(focusedScale = Focus.controlScale),
                colors = ClickableSurfaceDefaults.colors(
                    containerColor = IrisColors.Overlay06,
                    contentColor = IrisColors.Foreground,
                    focusedContainerColor = IrisColors.Overlay12,
                    focusedContentColor = IrisColors.Foreground,
                ),
            ) {
                Text(
                    "${c.flag} ${c.name}",
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(horizontal = Spacing.md, vertical = Spacing.sm),
                )
            }
        }
    }
}
