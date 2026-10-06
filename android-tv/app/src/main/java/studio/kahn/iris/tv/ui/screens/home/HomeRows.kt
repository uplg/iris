package studio.kahn.iris.tv.ui.screens.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.CardRow
import studio.kahn.iris.tv.ui.components.PosterCard
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.Spinner
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.StillCard
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** A card whose "Hold OK" menu is open, with its row's keys as they were (to find a neighbour once it is gone). */
data class OpenMenu(val card: CardModel, val eyebrow: String, val rowKeys: List<String>)

/**
 * The cards of a screen and their menu: one focus requester per card key (so the focus
 * can come back to a card, or its neighbour once the card left its row), and the open
 * [CardMenu]. A menu action that goes to the server keeps the menu open, busy, until the
 * screen's busy key clears ([settle]).
 */
@Stable
class CardFocus {
    private val requesters = HashMap<String, FocusRequester>()
    var menu by mutableStateOf<OpenMenu?>(null)
        private set
    var pending by mutableStateOf<CardAction?>(null)
        private set
    private var sawBusy = false
    private var refocus: OpenMenu? = null

    fun requester(key: String): FocusRequester = requesters.getOrPut(key) { FocusRequester() }

    fun open(card: CardModel, eyebrow: String, row: List<CardModel>) {
        menu = OpenMenu(card, eyebrow, row.map { it.key })
        pending = null
        sawBusy = false
    }

    /** The menu's action: [serverSide] ones keep it open until the server answered. */
    fun chose(action: CardAction, serverSide: Boolean) {
        if (serverSide) {
            pending = action
            sawBusy = false
        } else {
            dismiss()
        }
    }

    fun dismiss() {
        refocus = menu
        menu = null
        pending = null
    }

    /** Fed the screen's busy key: closes a pending menu once its action is over. */
    fun settle(busy: String?) {
        if (pending == null) return
        if (busy != null) {
            sawBusy = true
        } else if (sawBusy) {
            dismiss()
        }
    }

    /** After the menu closed: back to its card, or the nearest one left in its row, else [fallback]. */
    fun restore(present: (String) -> Boolean, fallback: FocusRequester?) {
        val m = refocus ?: return
        refocus = null
        val keys = m.rowKeys
        val at = keys.indexOf(m.card.key)
        val order = listOf(m.card.key) + keys.drop(at + 1) + keys.take(at.coerceAtLeast(0)).reversed()
        val target = order.firstOrNull(present)
        val done = target != null && runCatching { requester(target).requestFocus() }.isSuccess
        if (!done && fallback != null) runCatching { fallback.requestFocus() }
    }
}

@Composable
fun rememberCardFocus(): CardFocus = remember { CardFocus() }

/** The menu actions that wait for the server before the menu closes. */
val SERVER_ACTIONS = setOf(
    CardAction.GetAndPlay,
    CardAction.StartOver,
    CardAction.MarkWatched,
    CardAction.RemoveFromContinue,
    CardAction.RemoveFromWatchlist,
    CardAction.NotInterested,
)

/**
 * The open menu of [focus], if any, wired to [onCardAction]; [busy] is the screen's action
 * in flight. [present] says whether a card key is still on screen, [fallback] takes the
 * focus when its row emptied.
 */
@Composable
fun CardMenuHost(
    focus: CardFocus,
    busy: String?,
    present: (String) -> Boolean,
    fallback: FocusRequester?,
    onCardAction: (String, CardAction) -> Unit,
) {
    LaunchedEffect(busy) { focus.settle(busy) }
    val menu = focus.menu
    LaunchedEffect(menu) { if (menu == null) focus.restore(present, fallback) }
    if (menu != null) {
        CardMenu(
            eyebrow = menu.eyebrow,
            title = menu.card.title,
            actions = menu.card.menu,
            busy = focus.pending,
            onAction = { action ->
                focus.chose(action, serverSide = action in SERVER_ACTIONS)
                onCardAction(menu.card.key, action)
            },
            onDismiss = focus::dismiss,
        )
    }
}

/** One card: a 16:9 still ([still]) or a 2:3 poster ([fillCell] in a grid), OK = its primary action, hold OK = its menu. */
@Composable
fun HomeCard(
    card: CardModel,
    still: Boolean,
    focus: CardFocus,
    onAction: (String, CardAction) -> Unit,
    onMenu: (() -> Unit)?,
    modifier: Modifier = Modifier,
    fillCell: Boolean = false,
) {
    val m = modifier.focusRequester(focus.requester(card.key))
    val click = { onAction(card.key, card.primary) }
    val long = onMenu.takeIf { card.menu.isNotEmpty() }
    if (still) {
        StillCard(
            title = card.title,
            imageUrl = card.art,
            onClick = click,
            modifier = m,
            progress = card.progress,
            meta = card.meta,
            status = card.status,
            statusTone = card.tone,
            onLongClick = long,
        )
    } else {
        PosterCard(
            title = card.title,
            imageUrl = card.art,
            onClick = click,
            modifier = m,
            width = if (fillCell) null else IrisSize.posterRow,
            kind = card.kind,
            progress = card.progress,
            meta = card.meta,
            status = card.status,
            statusTone = card.tone,
            onLongClick = long,
        )
    }
}

/** A row heading: the title in Fraunces, a muted fact beside it, an optional "See all". */
@Composable
fun RowHead(title: String, meta: String?, onSeeAll: (() -> Unit)?, modifier: Modifier = Modifier) {
    Row(
        modifier.padding(horizontal = IrisLayout.current.safeHorizontal),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SectionTitle(title, meta = meta)
        if (onSeeAll != null) {
            ActionButton("See all", onSeeAll, style = ActionStyle.Secondary, size = ActionSize.Small)
        }
    }
}

/**
 * A row of a live list (web `Row`): the cards once there are some; before, under the same
 * title, the first load, the failure with a retry, or why it is empty and the way out.
 * Never a row that silently disappears. A stale list says so under its cards.
 */
@Composable
fun HomeRow(
    title: String,
    cards: Loadable<List<CardModel>>,
    focus: CardFocus,
    still: Boolean,
    onCardAction: (String, CardAction) -> Unit,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    meta: String? = null,
    onSeeAll: (() -> Unit)? = null,
    emptyText: String? = null,
    emptyHint: String? = null,
    emptyActionLabel: String? = null,
    onEmptyAction: (() -> Unit)? = null,
    rowFocus: FocusRequester? = null,
    hideEmpty: Boolean = false,
) {
    val list = cards.valueOrNull
    if (hideEmpty && list != null && list.isEmpty()) return
    val layout = IrisLayout.current
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
        RowHead(title, meta, onSeeAll)
        when {
            list != null && list.isNotEmpty() -> {
                CardRow(
                    items = list,
                    key = { it.key },
                    modifier = if (rowFocus != null) Modifier.focusRequester(rowFocus) else Modifier,
                ) { card ->
                    HomeCard(
                        card = card,
                        still = still,
                        focus = focus,
                        onAction = onCardAction,
                        onMenu = { focus.open(card, title, list) },
                    )
                }
                cards.errorOrNull?.let { StaleNotice(it, Modifier.padding(horizontal = layout.safeHorizontal)) }
            }
            list != null -> Column(
                Modifier.padding(horizontal = layout.safeHorizontal),
                verticalArrangement = Arrangement.spacedBy(IrisSpace.s2),
            ) {
                if (emptyText != null) Text(emptyText, style = IrisType.body, color = IrisColor.ink)
                if (emptyHint != null) Text(emptyHint, style = IrisType.meta, color = IrisColor.inkMuted)
                if (emptyActionLabel != null && onEmptyAction != null) {
                    ActionButton(
                        emptyActionLabel,
                        onEmptyAction,
                        style = ActionStyle.Secondary,
                        size = ActionSize.Small,
                        modifier = Modifier.padding(top = IrisSpace.s1),
                    )
                }
            }
            else -> RowState(cards, onRetry, Modifier.padding(horizontal = layout.safeHorizontal))
        }
    }
}

/** A row's first load, or its failure with a retry, inline under its title. */
@Composable
fun RowState(state: Loadable<*>, onRetry: () -> Unit, modifier: Modifier = Modifier) {
    val failed = state as? Loadable.Failed
    Row(
        modifier.semantics { liveRegion = LiveRegionMode.Polite },
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (failed == null) {
            Spinner(Modifier.size(12.dp), color = IrisColor.accent)
            Text("Loading…", style = IrisType.meta, color = IrisColor.inkMuted)
        } else {
            StatusLine("Couldn't load this: ${failed.error.message}", tone = StatusTone.Down)
            ActionButton("Try again", onRetry, icon = Icons.Rounded.Refresh, style = ActionStyle.Secondary, size = ActionSize.Small)
        }
    }
}
