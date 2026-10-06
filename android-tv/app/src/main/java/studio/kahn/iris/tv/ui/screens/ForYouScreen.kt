package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.ui.Modifier
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.screens.home.CardAction
import studio.kahn.iris.tv.ui.screens.home.CardFocus
import studio.kahn.iris.tv.ui.screens.home.HomeRow
import studio.kahn.iris.tv.ui.screens.home.RowState
import studio.kahn.iris.tv.ui.screens.home.ShelfModel
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * Every suggestion shelf (web `discover/ForYou`; the home shows a few): what is trending,
 * checked against the trackers. Then where the trends come from, as TMDB's terms ask.
 */
fun LazyListScope.forYouShelves(
    shelves: Loadable<List<ShelfModel>>,
    focus: CardFocus,
    onCardAction: (String, CardAction) -> Unit,
    onRetry: () -> Unit,
) {
    val list = shelves.valueOrNull
    when {
        list == null -> item(key = "fy-state") {
            RowState(shelves, onRetry, Modifier.padding(horizontal = IrisLayout.current.safeHorizontal))
        }
        list.isEmpty() -> item(key = "fy-empty") {
            Empty(
                "Nothing suggested yet.",
                "Discovery follows what is trending and checks it against your trackers every few hours. Check back soon.",
            )
        }
        else -> {
            items(list, key = { it.key }, contentType = { "row" }) { shelf ->
                HomeRow(
                    title = shelf.title,
                    cards = Loadable.Ready(shelf.cards),
                    focus = focus,
                    still = false,
                    onCardAction = onCardAction,
                    onRetry = onRetry,
                )
            }
            shelves.errorOrNull?.let { error ->
                item(key = "fy-stale") {
                    StaleNotice(error, Modifier.padding(horizontal = IrisLayout.current.safeHorizontal))
                }
            }
        }
    }
    item(key = "fy-credit") {
        Text(
            "Trends from TMDB and SIMKL, matched against your trackers. " +
                "This product uses the TMDB API but is not endorsed or certified by TMDB.",
            style = IrisType.meta,
            color = IrisColor.inkMuted,
            modifier = Modifier.padding(horizontal = IrisLayout.current.safeHorizontal),
        )
    }
}
