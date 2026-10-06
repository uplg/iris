package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
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
