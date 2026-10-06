package studio.kahn.iris.tv.ui.screens

import studio.kahn.iris.tv.ui.components.PosterAside
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.ui.components.EmptyState
import studio.kahn.iris.tv.ui.components.ErrorState
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.FocusReturn
import studio.kahn.iris.tv.ui.components.FooterLayout
import studio.kahn.iris.tv.ui.components.rememberFocusReturn
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.ScreenFooter
import studio.kahn.iris.tv.ui.components.LoadingState
import studio.kahn.iris.tv.ui.components.PillChoice
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.screens.search.AudioOption
import studio.kahn.iris.tv.ui.screens.search.GrabAsk
import studio.kahn.iris.tv.ui.screens.search.GrabRefusal
import studio.kahn.iris.tv.ui.screens.search.GrabUi
import studio.kahn.iris.tv.ui.components.LoadMoreAtEnd
import studio.kahn.iris.tv.ui.screens.search.MoreFooter
import studio.kahn.iris.tv.ui.screens.search.LibraryMatchRow
import studio.kahn.iris.tv.ui.screens.search.MatchTarget
import studio.kahn.iris.tv.ui.screens.search.ReturnFocusAfterGrab
import studio.kahn.iris.tv.ui.screens.search.matchKey
import studio.kahn.iris.tv.ui.components.focusReturn
import androidx.compose.foundation.lazy.items
import studio.kahn.iris.tv.ui.components.RowList
import studio.kahn.iris.tv.ui.screens.search.SearchKind
import studio.kahn.iris.tv.ui.screens.search.SearchSort
import studio.kahn.iris.tv.ui.screens.search.SeasonOption
import studio.kahn.iris.tv.ui.screens.search.TitleReleasesUiState
import studio.kahn.iris.tv.ui.screens.search.TitleReleasesViewModel
import studio.kahn.iris.tv.ui.screens.search.SummaryLine
import studio.kahn.iris.tv.ui.screens.search.failedTrackers
import studio.kahn.iris.tv.ui.format.languageLabel
import studio.kahn.iris.tv.ui.screens.search.releaseKey
import studio.kahn.iris.tv.ui.screens.search.releaseRows
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** What a title's releases screen can ask for; the defaults do nothing (screenshots). */
@Immutable
data class TitleReleasesActions(
    val onRetry: () -> Unit = {},
    val onLoadMore: () -> Unit = {},
    val onShowMore: () -> Unit = {},
    val onRetryMore: () -> Unit = {},
    val onLanguage: (String?) -> Unit = {},
    val onSeason: (Int?) -> Unit = {},
    val onRelease: (SearchResult) -> Unit = {},
    val onGrab: (SearchResult) -> Unit = {},
    val onConfirmGrab: () -> Unit = {},
    val onDismissGrab: () -> Unit = {},
    val onMatch: (MatchTarget) -> Unit = {},
)

/** A title picked in the Titles view and its releases on the trackers (TVTitleReleases). */
@Composable
fun TitleReleasesScreen(
    container: AppContainer,
    query: String,
    tmdbId: Long,
    kind: SearchKind,
    sort: SearchSort,
    onOpenRelease: (providerId: String, externalId: String, tmdbId: Long?, kind: String?) -> Unit,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    onOpenCollection: (collectionId: String) -> Unit,
) {
    val vm = irisViewModel(container) { c, _ -> TitleReleasesViewModel(c, query, tmdbId, kind, sort) }
    val state by vm.state.collectAsStateWithLifecycle()
    val grab by vm.grabber.state.collectAsStateWithLifecycle()
    val play by vm.grabber.play.collectAsStateWithLifecycle()
    LaunchedEffect(play) {
        play?.let {
            vm.grabber.played()
            onPlay(it.infohash, it.fileIdx)
        }
    }
    val actions = remember(vm) {
        TitleReleasesActions(
            onRetry = vm::retry,
            onLoadMore = vm::loadMore,
            onShowMore = vm::showMore,
            onRetryMore = vm::retryMore,
            onLanguage = vm::setLanguage,
            onSeason = vm::setSeason,
            onRelease = { r -> onOpenRelease(r.providerId, r.externalId, r.titleMatch?.tmdbId ?: tmdbId, (r.titleMatch?.kind ?: r.kind)?.value) },
            onGrab = vm::grab,
            onConfirmGrab = vm.grabber::confirm,
            onDismissGrab = vm.grabber::dismiss,
            onMatch = { target ->
                when (target) {
                    is MatchTarget.Play -> onPlay(target.infohash, target.fileIdx)
                    is MatchTarget.Open -> onOpenCollection(target.collectionId)
                }
            },
        )
    }
    TitleReleasesContent(state, grab, actions)
}

@Composable
fun TitleReleasesContent(state: TitleReleasesUiState, grab: GrabUi, actions: TitleReleasesActions) {
    val layout = IrisLayout.current
    val remembered = rememberFocusReturn()
    ReturnFocusAfterGrab(grab, remembered)
    val head = state.head
    // The first release once they arrive; coming back from one, the row left.
    val first = state.shown.firstOrNull()?.let(::releaseKey)
    LaunchedEffect(first != null) {
        if (first == null) return@LaunchedEffect
        withFrameNanos { }
        withFrameNanos { }
        remembered.focus(listOfNotNull(remembered.last ?: first))
    }
    val narrow = layout.narrow

    FooterLayout(
        footer = {
            ScreenFooter(
                listOf(
                    KeyHint(Keys.OK, "See the release"),
                    KeyHint(Keys.HOLD_OK, "Download and play"),
                    KeyHint(Keys.BACK, "To the search results"),
                ),
            )
        },
        modifier = Modifier
            .fillMaxSize()
            .background(IrisColor.ground),
    ) { footer ->
        Row(
            Modifier
                .fillMaxSize()
                .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, top = layout.safeVertical, bottom = footer),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s9),
        ) {
            PosterAside(
                above = "Search · ${state.query}",
                title = head.title,
                imageUrl = head.posterUrl,
                compact = narrow,
                modifier = Modifier.width(if (narrow) IrisSize.posterAside else IrisSize.asideColumn),
            ) {
                Text(head.title, style = if (narrow) IrisType.panel else IrisType.title, color = IrisColor.ink, maxLines = 3, overflow = TextOverflow.Ellipsis)
                head.meta?.let { Text(it, style = IrisType.meta, color = IrisColor.inkMuted) }
                if (head.inLibrary) StatusLine("In your library", tone = StatusTone.Ok)
            }
            Column(
                Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(IrisSpace.s4),
            ) {
                SectionTitle("Releases on the trackers", style = IrisType.panel)
                SummaryLine(state.summaryLine, failedTrackers(state.results.valueOrNull?.providers.orEmpty()), actions.onRetry)
                if (state.rows.isNotEmpty() || state.language != null || state.season != null) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .horizontalScroll(rememberScrollState())
                            .padding(4.dp),
                        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        val seasons = state.seasons
                        if (seasons.isNotEmpty()) {
                            PillChoice(
                                seasons,
                                seasons.firstOrNull { it.season == state.season } ?: seasons.first(),
                                { o: SeasonOption -> actions.onSeason(o.season) },
                                label = { it.label },
                            )
                            Box(
                                Modifier
                                    .padding(horizontal = IrisSpace.s1)
                                    .width(1.dp)
                                    .height(18.dp)
                                    .background(IrisColor.line, IrisShape.pill),
                            )
                        }
                        val audio = state.audio
                        PillChoice(
                            audio,
                            audio.firstOrNull { it.tag == state.language } ?: audio.first(),
                            { o: AudioOption -> actions.onLanguage(o.tag) },
                            label = { it.words },
                        )
                    }
                }
                GrabRefusal(grab, actions.onDismissGrab)
                Body(state, grab, actions, remembered)
            }
        }
        GrabAsk(grab, onConfirm = actions.onConfirmGrab, onCancel = actions.onDismissGrab)
    }
}

@Composable
private fun Body(
    state: TitleReleasesUiState,
    grab: GrabUi,
    actions: TitleReleasesActions,
    remembered: FocusReturn,
) {
    val bottom = IrisSpace.s3
    when (val results = state.results) {
        Loadable.Loading -> LoadingState(Modifier.padding(bottom = bottom), "Asking the trackers…")
        is Loadable.Failed -> ErrorState(results.error.message, actions.onRetry, Modifier.padding(bottom = bottom), title = "The search failed")
        else -> {
            results.valueOrNull ?: return
            if (state.shown.isEmpty() && state.matches.isEmpty()) {
                val language = languageLabel(state.language, long = false)
                EmptyState(
                    if (language != null) "No release in $language among the ${state.rows.size} loaded" else "No tracker has a release of ${state.head.title}",
                    Modifier.padding(bottom = bottom),
                    body = if (language != null) "Choose another language or season." else "Try the search again later: trackers get new uploads every day.",
                )
                return
            }
            val list = rememberLazyListState()
            LoadMoreAtEnd(list, enabled = state.pages.autoLoads, onLoadMore = actions.onLoadMore)
            RowList(list, contentPadding = PaddingValues(start = 4.dp, end = 4.dp, top = 4.dp, bottom = bottom)) {
                items(state.matches, key = ::matchKey) { m ->
                    LibraryMatchRow(m, actions.onMatch, Modifier.focusReturn(remembered, matchKey(m)))
                }
                releaseRows(state.shown, grab, remembered, actions.onRelease, actions.onGrab, withTitle = false)
                item(key = "more") { MoreFooter(state.pages, actions.onRetryMore, actions.onShowMore) }
            }
        }
    }
}
