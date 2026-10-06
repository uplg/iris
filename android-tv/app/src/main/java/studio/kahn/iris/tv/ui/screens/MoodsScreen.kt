package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.PillChoice
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.components.StillCard
import studio.kahn.iris.tv.ui.screens.home.CardAction
import studio.kahn.iris.tv.ui.screens.home.CardFocus
import studio.kahn.iris.tv.ui.screens.home.HomeCard
import studio.kahn.iris.tv.ui.screens.home.MoodModel
import studio.kahn.iris.tv.ui.screens.home.MoodResultsModel
import studio.kahn.iris.tv.ui.screens.home.RowState
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/*
 * Tonight's moods, the first section of Discover: the heading with the kind (movies or
 * series), then the board of moods or one mood's titles. Lazy-list sections, so the whole
 * Discover page scrolls as one list.
 */

private val KINDS = listOf(MediaKind.movie, MediaKind.tv)
private fun kindWords(kind: MediaKind) = if (kind == MediaKind.tv) "Series" else "Movies"

/** The narrowest mood tile: 4 columns on a TV, 3 on a phone. */
private val MOOD_TILE_MIN = 200.dp

/** The heading of the moods and the movies / series choice. */
fun LazyListScope.moodsHead(kind: MediaKind, onKind: (MediaKind) -> Unit, kindFocus: FocusRequester) {
    item(key = "moods-head", contentType = "head") {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = IrisLayout.current.safeHorizontal),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s6),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SectionTitle("What are you in the mood for?", style = IrisType.panel)
            PillChoice(
                options = KINDS,
                selected = kind,
                onSelect = onKind,
                label = ::kindWords,
                modifier = Modifier.focusRequester(kindFocus),
            )
        }
    }
}

/**
 * The curated moods for the kind, the account's genres first: each a tile with the
 * backdrop of its top title, its name, and that title in words.
 */
fun LazyListScope.moodBoard(
    board: Loadable<List<MoodModel>>,
    onOpen: (MoodModel) -> Unit,
    onRetry: () -> Unit,
    tileFocus: (String) -> FocusRequester,
    columns: Int,
) {
    val moods = board.valueOrNull
    when {
        moods == null -> item(key = "moods-state") { SectionState(board, onRetry) }
        moods.isEmpty() -> item(key = "moods-empty") {
            Empty("No mood has anything to get right now.", "Moods fill in as your trackers carry new releases.")
        }
        else -> {
            moods.chunked(columns).forEachIndexed { row, tiles ->
                item(key = "moods-row-$row-${tiles.first().id}", contentType = "mood-row") {
                    GridRow(columns, tiles) { mood ->
                        StillCard(
                            title = mood.label,
                            imageUrl = mood.art,
                            onClick = { onOpen(mood) },
                            width = null,
                            meta = mood.now,
                            modifier = Modifier.focusRequester(tileFocus(mood.id)),
                        )
                    }
                }
            }
            board.errorOrNull?.let { error -> item(key = "moods-stale") { Stale(error) } }
        }
    }
}

/** One mood's titles for the kind: what the trackers carry, recent first, tuned to the account. */
fun LazyListScope.moodResults(
    mood: MoodResultsModel,
    focus: CardFocus,
    onBackToMoods: () -> Unit,
    onCardAction: (String, CardAction) -> Unit,
    onRetry: () -> Unit,
    columns: Int,
) {
    item(key = "mood-title", contentType = "head") {
        Column(
            Modifier.padding(horizontal = IrisLayout.current.safeHorizontal),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        ) {
            ActionButton(
                "All moods",
                onBackToMoods,
                icon = Icons.AutoMirrored.Rounded.ArrowBack,
                style = ActionStyle.Secondary,
                size = ActionSize.Small,
            )
            SectionTitle(mood.label, meta = mood.count)
        }
    }
    val cards = mood.cards.valueOrNull
    when {
        cards == null -> item(key = "mood-state") { SectionState(mood.cards, onRetry) }
        cards.isEmpty() -> item(key = "mood-empty") {
            Empty("Nothing to get for this mood right now.", "Try another mood, or the other kind.")
        }
        else -> {
            cards.chunked(columns).forEachIndexed { row, chunk ->
                item(key = "mood-row-$row-${chunk.first().key}", contentType = "poster-row") {
                    GridRow(columns, chunk) { card ->
                        HomeCard(
                            card = card,
                            still = false,
                            focus = focus,
                            onAction = onCardAction,
                            onMenu = { focus.open(card, mood.label, cards) },
                            fillCell = true,
                        )
                    }
                }
            }
            mood.cards.errorOrNull?.let { error -> item(key = "mood-stale") { Stale(error) } }
        }
    }
}

/** How many mood tiles, or posters, fit a row. */
@Composable
fun moodColumns(): Int = IrisLayout.current.columns(MOOD_TILE_MIN, IrisSpace.s6)

@Composable
fun posterColumns(): Int = IrisLayout.current.columns(IrisSize.posterGridMin, IrisSpace.s6)

/** A grid line inside the page's list: [columns] equal cells, the last line's gaps left empty. */
@Composable
private fun <T> GridRow(columns: Int, items: List<T>, gap: Dp = IrisSpace.s6, cell: @Composable (T) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = IrisLayout.current.safeHorizontal)
            .focusGroup(),
        horizontalArrangement = Arrangement.spacedBy(gap),
    ) {
        items.forEach { item -> Column(Modifier.weight(1f)) { cell(item) } }
        repeat(columns - items.size) { Spacer(Modifier.weight(1f)) }
    }
}

@Composable
private fun SectionState(state: Loadable<*>, onRetry: () -> Unit) {
    RowState(state, onRetry, Modifier.padding(horizontal = IrisLayout.current.safeHorizontal))
}

@Composable
private fun Stale(error: studio.kahn.iris.tv.ui.state.UiError) {
    StaleNotice(error, Modifier.padding(horizontal = IrisLayout.current.safeHorizontal))
}

@Composable
internal fun Empty(text: String, hint: String) {
    Column(
        Modifier.padding(horizontal = IrisLayout.current.safeHorizontal),
        verticalArrangement = Arrangement.spacedBy(IrisSpace.s2),
    ) {
        Text(text, style = IrisType.body, color = IrisColor.ink)
        Text(hint, style = IrisType.meta, color = IrisColor.inkMuted)
    }
}
