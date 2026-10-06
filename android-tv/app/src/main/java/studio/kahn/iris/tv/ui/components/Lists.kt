package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filter
import studio.kahn.iris.tv.ui.state.UiError
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisType
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyGridItemScope
import androidx.compose.foundation.lazy.grid.LazyGridScope
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.unit.Dp
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace

/**
 * A horizontal row of cards (Continue watching, a shelf). A foundation
 * [LazyRow] (never the deprecated TvLazy*), so:
 *
 * - [key] must be stable and unique per item (the row restores focus and
 *   scroll by key; a duplicate key crashes).
 * - Leaving the row and coming back with the D-pad lands on the card
 *   focused last ([focusRestorer]); the first time, on [initialFocus] if
 *   given (attach it to one item's modifier), else on the first card.
 * - The content padding defaults to the safe margins, which is also the room
 *   the focus ring and the 1.06 scale need at the row's ends.
 */
@Composable
fun <T> CardRow(
    items: List<T>,
    key: (T) -> Any,
    modifier: Modifier = Modifier,
    state: LazyListState = rememberLazyListState(),
    contentPadding: PaddingValues = PaddingValues(horizontal = IrisLayout.current.safeHorizontal),
    gap: Dp = IrisSpace.s6,
    initialFocus: FocusRequester = FocusRequester.Default,
    itemContent: @Composable LazyItemScope.(T) -> Unit,
) {
    LazyRow(
        modifier = modifier
            .focusRestorer(initialFocus)
            .focusGroup(),
        state = state,
        contentPadding = contentPadding,
        horizontalArrangement = Arrangement.spacedBy(gap),
    ) {
        items(items, key = key) { item -> itemContent(item) }
    }
}

/**
 * A vertical list of rows (search results as a list, a title's releases), with the [CardRow]
 * contract: stable keys, and coming back to the list lands on the row focused last.
 */
@Composable
fun RowList(
    state: LazyListState,
    modifier: Modifier = Modifier,
    contentPadding: PaddingValues = PaddingValues(4.dp),
    gap: Dp = 7.dp,
    content: LazyListScope.() -> Unit,
) {
    LazyColumn(
        modifier = modifier
            .focusRestorer()
            .focusGroup(),
        state = state,
        contentPadding = contentPadding,
        verticalArrangement = Arrangement.spacedBy(gap),
        content = content,
    )
}

/** Asks for the next page ([onLoadMore]) when the last rows of [state] come into view: driven by scrolling, no timer. */
@Composable
fun LoadMoreAtEnd(state: LazyListState, enabled: Boolean, onLoadMore: () -> Unit) {
    LoadMoreNear(state, enabled, LOOKAHEAD, onLoadMore) {
        state.layoutInfo.let { (it.visibleItemsInfo.lastOrNull()?.index ?: 0) to it.totalItemsCount }
    }
}

/** [LoadMoreAtEnd] for a grid: a row of a grid holds several items, so it looks further ahead. */
@Composable
fun LoadMoreAtEnd(state: LazyGridState, enabled: Boolean, onLoadMore: () -> Unit) {
    LoadMoreNear(state, enabled, LOOKAHEAD * 2, onLoadMore) {
        state.layoutInfo.let { (it.visibleItemsInfo.lastOrNull()?.index ?: 0) to it.totalItemsCount }
    }
}

@Composable
private fun LoadMoreNear(key: Any, enabled: Boolean, lookahead: Int, onLoadMore: () -> Unit, seen: () -> Pair<Int, Int>) {
    val load by rememberUpdatedState(onLoadMore)
    LaunchedEffect(key, enabled) {
        if (!enabled) return@LaunchedEffect
        snapshotFlow { seen().let { (last, total) -> last >= total - lookahead } }
            .distinctUntilChanged()
            .filter { it }
            .collect { load() }
    }
}

private const val LOOKAHEAD = 4

/**
 * The end of a paged list: the next page loading ([loadingText]), its failure with a retry,
 * or the end said ([endText]).
 */
@Composable
fun PageEnd(
    loadingMore: Boolean,
    error: UiError?,
    hasNext: Boolean,
    onRetry: () -> Unit,
    loadingText: String,
    endText: String,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier
            .fillMaxWidth()
            .heightIn(min = IrisSize.control),
        contentAlignment = Alignment.Center,
    ) {
        when {
            error != null -> Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3), verticalAlignment = Alignment.CenterVertically) {
                StatusLine("The next page did not load: ${error.message}", tone = StatusTone.Down)
                ActionButton("Try again", onRetry, style = ActionStyle.Secondary, size = ActionSize.Small, icon = Icons.Rounded.Refresh)
            }
            loadingMore || hasNext -> Row(
                Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Spinner(Modifier.size(10.dp), color = IrisColor.inkMuted)
                Text(loadingText, style = IrisType.meta, color = IrisColor.inkMuted)
            }
            else -> Text(endText, style = IrisType.meta, color = IrisColor.inkMuted)
        }
    }
}

/**
 * A poster grid (Library, search results as a grid). Columns are as many as
 * fit cells at least [minCell] wide in the width available (7 on a TV for
 * the default, fewer on a phone): never a fixed TV count.
 *
 * [header] adds full-width items above the posters (titles, filters); use
 * `item(span = { GridItemSpan(maxLineSpan) })` in it. Focus restores to
 * the poster focused last, as in [CardRow]; [key] must be stable.
 */
@Composable
fun <T> PosterGrid(
    items: List<T>,
    key: (T) -> Any,
    modifier: Modifier = Modifier,
    state: LazyGridState = rememberLazyGridState(),
    minCell: Dp = IrisSize.posterGridMin,
    contentPadding: PaddingValues = IrisLayout.current.let {
        PaddingValues(start = it.safeHorizontal, end = it.safeHorizontal, top = IrisSpace.s3, bottom = it.safeVertical)
    },
    horizontalGap: Dp = IrisSpace.s6,
    verticalGap: Dp = IrisSpace.s7,
    initialFocus: FocusRequester = FocusRequester.Default,
    header: (LazyGridScope.() -> Unit)? = null,
    itemContent: @Composable LazyGridItemScope.(T) -> Unit,
) {
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minCell),
        modifier = modifier
            .focusRestorer(initialFocus)
            .focusGroup(),
        state = state,
        contentPadding = contentPadding,
        horizontalArrangement = Arrangement.spacedBy(horizontalGap),
        verticalArrangement = Arrangement.spacedBy(verticalGap),
    ) {
        header?.invoke(this)
        items(items, key = key) { item -> itemContent(item) }
    }
}
