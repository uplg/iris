package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** Nothing to show yet: a spinner and the words for what is loading. */
@Composable
fun LoadingState(
    modifier: Modifier = Modifier,
    label: String = "Loading…",
) {
    Centered(modifier.semantics { liveRegion = LiveRegionMode.Polite }) {
        Spinner(Modifier.size(18.dp), color = IrisColor.accent)
        Text(label, style = IrisType.meta, color = IrisColor.inkMuted)
    }
}

/**
 * Nothing to show, and that is not an error ("No releases found"). [body]
 * says why or what to do; [actionLabel] + [onAction] offer the way out.
 */
@Composable
fun EmptyState(
    title: String,
    modifier: Modifier = Modifier,
    body: String? = null,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
) {
    Centered(modifier) {
        Text(title, style = IrisType.group, color = IrisColor.ink, textAlign = TextAlign.Center)
        if (body != null) Text(body, style = IrisType.meta, color = IrisColor.inkMuted, textAlign = TextAlign.Center)
        if (actionLabel != null && onAction != null) {
            ActionButton(actionLabel, onAction, style = ActionStyle.Secondary, modifier = Modifier.padding(top = IrisSpace.s2))
        }
    }
}

/**
 * A read failed and there is nothing to show: the error in words and a
 * Retry. Pass [retryFocus] to put the D-pad on Retry (when the error fills
 * the screen and nothing else is focusable).
 */
@Composable
fun ErrorState(
    message: String,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    title: String = "Couldn't load this",
    retryFocus: FocusRequester? = null,
) {
    Centered(modifier.semantics { liveRegion = LiveRegionMode.Polite }) {
        StatusLine(title, tone = StatusTone.Down, style = IrisType.body)
        Text(message, style = IrisType.meta, color = IrisColor.inkMuted, textAlign = TextAlign.Center)
        ActionButton(
            "Try again",
            onRetry,
            icon = Icons.Rounded.Refresh,
            style = ActionStyle.Secondary,
            modifier = Modifier
                .padding(top = IrisSpace.s2)
                .then(if (retryFocus != null) Modifier.focusRequester(retryFocus) else Modifier),
        )
    }
}

/**
 * Says a value on screen may be out of date because its last refresh
 * failed ([Loadable.Stale]). Put it near what it qualifies.
 */
@Composable
fun StaleNotice(error: UiError, modifier: Modifier = Modifier) {
    StatusLine(
        "Not up to date: ${error.message}",
        modifier = modifier.semantics { liveRegion = LiveRegionMode.Polite },
        tone = StatusTone.Warn,
    )
}

/**
 * The standard switch over a [Loadable]: [LoadingState], [ErrorState] with
 * Retry, or [content] with the value (for a stale value too: show
 * [StaleNotice] inside [content] from `state.errorOrNull`).
 */
@Composable
fun <T> LoadableContent(
    state: Loadable<T>,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    loadingLabel: String = "Loading…",
    content: @Composable (T) -> Unit,
) {
    when (state) {
        Loadable.Loading -> LoadingState(modifier, loadingLabel)
        is Loadable.Failed -> ErrorState(state.error.message, onRetry, modifier)
        is Loadable.Ready -> content(state.value)
        is Loadable.Stale -> content(state.value)
    }
}

@Composable
private fun Centered(modifier: Modifier, content: @Composable () -> Unit) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(
            Modifier.widthIn(max = 420.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        ) { content() }
    }
}
