package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.ColorMatrix
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import coil3.request.ImageRequest
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisFocus
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

private const val POSTER_ASPECT = 2f / 3f
private const val STILL_ASPECT = 16f / 9f
private val GREYED = ColorFilter.colorMatrix(ColorMatrix().apply { setToSaturation(0f) })

/**
 * A 2:3 poster card (Library, Search titles, Discover).
 *
 * - [width]: pass it in a row (the default, 200 px); pass null in a grid and
 *   the card fills its cell. Either way Coil decodes the artwork at the
 *   card's size, never the full TMDB image.
 * - With no [imageUrl] (or while it loads) the tile shows [title] in Fraunces
 *   on the fallback fill, and [kind] ("Series · 2022") above it.
 * - [badge] is laid over the artwork's corner (the provider in a release grid).
 * - Below: an optional [progress] meter (always say the value in [meta] or
 *   [status] too), the title line, [meta] (muted), then [status] in its tone.
 * - [dimmed]: the artwork greyed (a title no longer on disk); [status] says why.
 * - Focused: the card grows to 1.06 and the artwork wears the accent ring.
 *   [onLongClick] = hold OK / touch long press (the boards' "Hold OK" menu).
 */
@Composable
fun PosterCard(
    title: String,
    imageUrl: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    width: Dp? = IrisSize.posterRow,
    kind: String? = null,
    badge: String? = null,
    progress: Float? = null,
    meta: String? = null,
    status: String? = null,
    statusTone: StatusTone = StatusTone.Muted,
    dimmed: Boolean = false,
    onLongClick: (() -> Unit)? = null,
) {
    ArtCard(
        aspect = POSTER_ASPECT,
        fallbackTitle = IrisType.artTitle.copy(fontSize = 12.sp, lineHeight = 14.sp),
        title = title,
        imageUrl = imageUrl,
        onClick = onClick,
        modifier = modifier,
        width = width,
        kind = kind,
        badge = badge,
        progress = progress,
        meta = meta,
        status = status,
        statusTone = statusTone,
        dimmed = dimmed,
        onLongClick = onLongClick,
    )
}

/**
 * A 16:9 still card (Continue watching). Same contract as [PosterCard];
 * [width] defaults to the board's 384 px.
 */
@Composable
fun StillCard(
    title: String,
    imageUrl: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    width: Dp? = IrisSize.stillRow,
    badge: String? = null,
    progress: Float? = null,
    meta: String? = null,
    status: String? = null,
    statusTone: StatusTone = StatusTone.Muted,
    onLongClick: (() -> Unit)? = null,
) {
    ArtCard(
        aspect = STILL_ASPECT,
        fallbackTitle = IrisType.artTitle.copy(fontSize = 15.sp, lineHeight = 17.sp),
        title = title,
        imageUrl = imageUrl,
        onClick = onClick,
        modifier = modifier,
        width = width,
        kind = null,
        badge = badge,
        progress = progress,
        meta = meta,
        status = status,
        statusTone = statusTone,
        dimmed = false,
        onLongClick = onLongClick,
    )
}

@Composable
private fun ArtCard(
    aspect: Float,
    fallbackTitle: TextStyle,
    title: String,
    imageUrl: String?,
    onClick: () -> Unit,
    modifier: Modifier,
    width: Dp?,
    kind: String?,
    badge: String?,
    progress: Float?,
    meta: String?,
    status: String?,
    statusTone: StatusTone,
    dimmed: Boolean,
    onLongClick: (() -> Unit)?,
) {
    FocusSurface(
        onClick = onClick,
        onLongClick = onLongClick,
        modifier = if (width != null) modifier.width(width) else modifier.fillMaxWidth(),
        shape = IrisShape.card,
        colors = FocusColors.None,
        ringBounds = { size -> Rect(0f, 0f, size.width, size.width / aspect) },
        focusedScale = IrisFocus.cardScale,
        scaleOrigin = TransformOrigin(0.5f, 0f),
        contentAlignment = Alignment.TopStart,
    ) { focused ->
        Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
            Artwork(
                title = title,
                imageUrl = imageUrl,
                aspect = aspect,
                width = width,
                kind = kind,
                badge = badge,
                framed = !focused,
                titleStyle = fallbackTitle,
                dimmed = dimmed,
            )
            if (progress != null) Meter(progress)
            Text(title, style = IrisType.bodyStrong, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (meta != null) {
                Text(meta, style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            // A narrow poster cuts "In progress · S1:E7 · 30 min left" short on one line.
            if (status != null) StatusLine(status, tone = statusTone, maxLines = 2)
        }
    }
}

/**
 * Artwork with its fallback: the fill and the [title] in Fraunces ([IrisType.artTitle]) are drawn
 * first, the image on top once decoded. Use it directly for a non-focusable
 * poster (a page's aside, the getting-ready screen); [width] lets Coil
 * decode at that size. [showTitle] false for a mini poster beside a row.
 * [dimmed]: greyed and faded (web `ghost-card`), for what is no longer on disk.
 */
@Composable
fun Artwork(
    title: String,
    imageUrl: String?,
    modifier: Modifier = Modifier,
    aspect: Float = POSTER_ASPECT,
    width: Dp? = null,
    kind: String? = null,
    badge: String? = null,
    framed: Boolean = true,
    titleStyle: TextStyle = IrisType.artTitle,
    showTitle: Boolean = true,
    dimmed: Boolean = false,
) {
    val shape = IrisShape.card
    Box(
        (if (width != null) modifier.width(width) else modifier.fillMaxWidth())
            .aspectRatio(aspect)
            .graphicsLayer { alpha = if (dimmed) DISABLED_ALPHA else 1f }
            .clip(shape)
            .background(IrisColor.art)
            .then(if (framed) Modifier.border(1.dp, IrisColor.line, shape) else Modifier),
    ) {
        if (showTitle) {
            Column(
                Modifier
                    .matchParentSize()
                    .padding(7.dp),
                verticalArrangement = Arrangement.SpaceBetween,
            ) {
                // The badge takes the top corner when there is one.
                Text(
                    if (badge == null) kind?.uppercase().orEmpty() else "",
                    style = IrisType.metaSmall.copy(letterSpacing = IrisType.eyebrow.letterSpacing),
                    color = IrisColor.artInk.copy(alpha = 0.8f),
                    maxLines = 1,
                )
                Text(title, style = titleStyle, color = IrisColor.artInk, maxLines = 3, overflow = TextOverflow.Ellipsis)
            }
        }
        if (!imageUrl.isNullOrBlank()) {
            val context = LocalContext.current
            val density = LocalDensity.current
            val request = remember(imageUrl, width, aspect, density) {
                ImageRequest.Builder(context)
                    .data(imageUrl)
                    .apply {
                        if (width != null) {
                            val w = with(density) { width.roundToPx() }
                            size(w, (w / aspect).toInt())
                        }
                    }
                    .build()
            }
            AsyncImage(
                model = request,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                colorFilter = if (dimmed) GREYED else null,
                modifier = Modifier.matchParentSize(),
            )
        }
        if (badge != null) {
            Text(
                badge,
                style = IrisType.metaSmall.copy(fontSize = 9.sp),
                color = IrisColor.ink,
                maxLines = 1,
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .padding(6.dp)
                    .background(IrisColor.artScrim, IrisShape.pill)
                    .padding(horizontal = 5.dp, vertical = 3.dp),
            )
        }
    }
}

/**
 * A framed, focusable row (search results as a list, a title's releases):
 * surface fill and a line at rest; focused, the raised fill and the accent
 * ring. Lay the row's content out in [content] (a mini [Artwork], text
 * lines, trailing chips).
 */
@Composable
fun RowCard(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    onLongClick: (() -> Unit)? = null,
    enabled: Boolean = true,
    content: @Composable RowScope.(focused: Boolean) -> Unit,
) {
    FocusSurface(
        onClick = onClick,
        onLongClick = onLongClick,
        enabled = enabled,
        modifier = modifier.fillMaxWidth(),
        shape = IrisShape.card,
        colors = FocusColors.Row,
        contentAlignment = Alignment.CenterStart,
    ) { focused ->
        Row(
            Modifier.padding(horizontal = 12.dp, vertical = 9.dp),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            content(focused)
        }
    }
}

/**
 * A non-focusable framed group (release notes, a settings group, the
 * "In your library" match): surface fill, a line, the panel radius. The
 * controls inside take focus themselves.
 */
@Composable
fun FramedBlock(
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(
        modifier
            .clip(IrisShape.panel)
            .background(IrisColor.surface)
            .border(1.dp, IrisColor.line, IrisShape.panel)
            .padding(horizontal = 13.dp, vertical = 11.dp),
        verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        content = content,
    )
}

/**
 * The one poster aside of a page (a title, a release, a title's releases): a line above it
 * ([above], an eyebrow), the poster with its title in Cal Sans when the art is missing, then
 * [below]. [compact] on a short or narrow screen: the smaller poster.
 */
@Composable
fun PosterAside(
    above: String,
    title: String,
    imageUrl: String?,
    compact: Boolean,
    modifier: Modifier = Modifier,
    below: @Composable ColumnScope.() -> Unit = {},
) {
    val poster = if (compact) IrisSize.posterAsideCompact else IrisSize.posterAside
    Column(modifier.widthIn(min = poster), verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
        Eyebrow(above)
        Artwork(title = title, imageUrl = imageUrl, width = poster, titleStyle = IrisType.group)
        below()
    }
}
