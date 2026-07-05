// File-level opt-in: PlayerView / MediaItem / buildPlayer are all
// @UnstableApi; `@OptIn` doesn't propagate into AndroidView lambdas.
@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package studio.kahn.iris.tv.ui.screens

import android.view.KeyEvent
import android.view.ViewGroup
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.PlayerView
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveNowNext
import studio.kahn.iris.tv.data.buildMediaItem
import studio.kahn.iris.tv.data.buildPlayer
import studio.kahn.iris.tv.data.humanizePlaybackError
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing

/** Now/next refresh cadence while watching (drives the overlay). */
private const val EPG_REFRESH_MS = 30_000L

/** How long the channel-name strip stays up after it (re)appears. */
private const val OVERLAY_VISIBLE_MS = 4_000L

/** Automatic reconnect attempts on a playback error before giving up to the
 *  Retry UI. Each attempt first tells the backend to demote the dead source,
 *  then reloads — so the retries walk through a channel's fallback feeds. */
private const val MAX_AUTO_RETRIES = 3

/**
 * Live channel playback. Deliberately NOT [WatchScreen] — that one is
 * torrent-coupled (probe, /play/status gating, saved progress, episode
 * nav). Live TV is a plain HLS stream from the backend proxy:
 * [buildMediaItem] routes the `.m3u8` URL to `HlsMediaSource`, and
 * [AppContainer.mediaOkHttpClient] brings cookie auth + transparent 401
 * refresh.
 *
 * DPAD up/down zaps to the previous/next channel of the country's list.
 * On a playback error the screen re-prepares once automatically — the
 * backend rotates to the channel's next upstream source on that reload —
 * then surfaces the error with a Retry button.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun LiveTvWatchScreen(
    container: AppContainer,
    country: String,
    initialChannelId: String,
    onBack: () -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current

    var serverUrl by remember { mutableStateOf<String?>(null) }
    var channels by remember { mutableStateOf<List<LiveChannel>>(emptyList()) }
    var channelId by remember { mutableStateOf(initialChannelId) }
    var epg by remember { mutableStateOf<Map<String, LiveNowNext>>(emptyMap()) }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    // Automatic reconnects used up on THIS channel (reset when zapping away).
    var autoRetryCount by remember(channelId) { mutableStateOf(0) }
    // Bumped to force a re-prepare of the current channel (Retry button + the
    // post-demotion auto-reconnect).
    var retryNonce by remember { mutableStateOf(0) }
    // First video frame rendered for the current (channel, attempt)? Drives the
    // "Connecting…" placeholder so a reconnect isn't a silent black screen.
    var rendered by remember(channelId, retryNonce) { mutableStateOf(false) }
    // Compose owns focus + input on this screen (the PlayerView is pure
    // display). `rootFocus` holds focus during playback so DPAD zapping works;
    // `retryFocus` takes it when the error card appears so its buttons are
    // reachable — the exact bug where "Back to channels" couldn't be focused.
    val rootFocus = remember { FocusRequester() }
    val retryFocus = remember { FocusRequester() }

    // Channel-name/now-next strip auto-hides a few seconds after it appears —
    // it must not sit persistently over the picture. `overlayTick` is bumped
    // to (re)show it: on a zap (channelId change) and on any remote key.
    var overlayVisible by remember { mutableStateOf(true) }
    var overlayTick by remember { mutableStateOf(0) }
    LaunchedEffect(channelId, overlayTick) {
        overlayVisible = true
        delay(OVERLAY_VISIBLE_MS)
        overlayVisible = false
    }

    LaunchedEffect(Unit) {
        serverUrl = container.sessionStore.serverUrl.first()
        val url = serverUrl ?: return@LaunchedEffect
        runCatching { container.apiFor(url).liveTvChannels(country) }
            .onSuccess { channels = it.channels }
    }
    LaunchedEffect(serverUrl) {
        val url = serverUrl ?: return@LaunchedEffect
        while (true) {
            runCatching { container.apiFor(url).liveTvEpgNow(country) }
                .onSuccess { res -> epg = res.propertyEntries.associateBy { it.channelId } }
            delay(EPG_REFRESH_MS)
        }
    }

    val player = remember { mutableStateOf<ExoPlayer?>(null) }

    // (Re)load the stream whenever the channel changes or Retry is pressed.
    LaunchedEffect(channelId, serverUrl, retryNonce) {
        val url = serverUrl ?: return@LaunchedEffect
        errorMessage = null
        val base = if (url.endsWith("/")) url else "$url/"
        val masterUrl = "${base}api/livetv/$country/channels/$channelId/master.m3u8"
        val name = channels.firstOrNull { it.id == channelId }?.name ?: channelId
        val p = player.value ?: buildPlayer(context, container.mediaOkHttpClient).also {
            player.value = it
        }
        p.setMediaItem(buildMediaItem(masterUrl, name))
        p.prepare()
        p.playWhenReady = true
    }

    // Error listener: on failure, tell the backend to demote the dead source
    // and — crucially — WAIT for that POST to land before bumping `retryNonce`
    // to reload. The old code re-prepared immediately, racing the async demote,
    // so the reload usually got the same dead source back (why M6 never
    // recovered). We walk up to MAX_AUTO_RETRIES feeds this way, then surface
    // the Retry card. `onRenderedFirstFrame` clears the "Connecting…" state.
    DisposableEffect(player.value) {
        val p = player.value ?: return@DisposableEffect onDispose {}
        val listener = object : Player.Listener {
            override fun onPlayerError(error: PlaybackException) {
                val url = serverUrl
                if (url != null && autoRetryCount < MAX_AUTO_RETRIES) {
                    autoRetryCount++
                    container.applicationScope.launch {
                        runCatching { container.apiFor(url).liveTvPlaybackError(country, channelId) }
                        // Source is demoted now → reload elects the next feed.
                        retryNonce++
                    }
                } else {
                    url?.let {
                        container.applicationScope.launch {
                            runCatching { container.apiFor(it).liveTvPlaybackError(country, channelId) }
                        }
                    }
                    errorMessage = humanizePlaybackError(error).first
                }
            }

            override fun onRenderedFirstFrame() {
                rendered = true
            }
        }
        p.addListener(listener)
        onDispose { p.removeListener(listener) }
    }

    DisposableEffect(Unit) {
        onDispose {
            player.value?.release()
            player.value = null
        }
    }

    val zap: (Int) -> Unit = zap@{ delta ->
        if (channels.isEmpty()) return@zap
        val idx = channels.indexOfFirst { it.id == channelId }.coerceAtLeast(0)
        val next = channels[(idx + delta + channels.size) % channels.size]
        channelId = next.id
    }

    // Focus routing: the error card's buttons take focus when it appears (so
    // "Back to channels" is reachable), and focus returns to the root — which
    // handles zapping — the rest of the time.
    LaunchedEffect(errorMessage) {
        runCatching {
            if (errorMessage != null) retryFocus.requestFocus() else rootFocus.requestFocus()
        }
    }

    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black)
            .onPreviewKeyEvent { event ->
                if (event.nativeKeyEvent.action != KeyEvent.ACTION_DOWN) {
                    return@onPreviewKeyEvent false
                }
                // Any remote press brings the channel strip back for a beat.
                overlayTick++
                // Preview phase: DPAD/CHANNEL up-down zap even while the error
                // card's buttons hold focus (letting the viewer escape a dead
                // channel); left/right/center fall through to those buttons.
                when (event.nativeKeyEvent.keyCode) {
                    KeyEvent.KEYCODE_CHANNEL_UP, KeyEvent.KEYCODE_DPAD_UP -> {
                        zap(-1)
                        true
                    }
                    KeyEvent.KEYCODE_CHANNEL_DOWN, KeyEvent.KEYCODE_DPAD_DOWN -> {
                        zap(1)
                        true
                    }
                    else -> false
                }
            }
            .focusRequester(rootFocus)
            .focusable(),
    ) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { ctx ->
                PlayerView(ctx).apply {
                    this.player = player.value
                    // Pure display — Compose owns focus + input. Leaving the
                    // controller focusable is what trapped D-pad focus on the
                    // dead player and stranded the error card.
                    useController = false
                    isFocusable = false
                    descendantFocusability = ViewGroup.FOCUS_BLOCK_DESCENDANTS
                    keepScreenOn = true
                    layoutParams = android.widget.FrameLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT,
                        ViewGroup.LayoutParams.MATCH_PARENT,
                    )
                }
            },
            update = { it.player = player.value },
        )

        // "Connecting…" while a (re)load hasn't rendered its first frame yet,
        // so an auto-reconnect isn't a silent black screen.
        if (errorMessage == null && !rendered) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(
                    "Connecting…",
                    style = MaterialTheme.typography.bodyMedium,
                    color = IrisColors.MutedForeground,
                )
            }
        }

        // Channel name + now/next strip: shown on entry / zap / any key, then
        // auto-hidden so it doesn't sit persistently over the picture.
        if (overlayVisible) {
            ChannelOverlay(
                channel = channels.firstOrNull { it.id == channelId },
                fallbackName = channelId,
                nowNext = epg[channelId],
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .padding(Spacing.xl),
            )
        }

        if (errorMessage != null) {
            Box(
                Modifier
                    .fillMaxSize()
                    .background(Color.Black.copy(alpha = 0.75f)),
                contentAlignment = Alignment.Center,
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(Spacing.md),
                ) {
                    Text(
                        "Stream unavailable",
                        style = MaterialTheme.typography.titleMedium,
                        color = IrisColors.Foreground,
                    )
                    Text(
                        errorMessage ?: "",
                        style = MaterialTheme.typography.bodyMedium,
                        color = IrisColors.MutedForeground,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
                        IrisButton(
                            "Retry",
                            onClick = {
                                autoRetryCount = 0
                                errorMessage = null
                                retryNonce++
                            },
                            modifier = Modifier.focusRequester(retryFocus),
                        )
                        IrisButton(
                            "Back to channels",
                            onClick = onBack,
                            variant = studio.kahn.iris.tv.ui.components.IrisButtonVariant.Ghost,
                        )
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ChannelOverlay(
    channel: LiveChannel?,
    fallbackName: String,
    nowNext: LiveNowNext?,
    modifier: Modifier = Modifier,
) {
    val now = nowNext?.now
    Column(
        modifier
            .background(Color.Black.copy(alpha = 0.55f), RoundedCornerShape(Radius.lg))
            .padding(horizontal = Spacing.lg, vertical = Spacing.md),
        verticalArrangement = Arrangement.spacedBy(Spacing.xs),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.sm),
        ) {
            val badge = channel?.tntNumber?.toString()
            if (badge != null) {
                Text(
                    badge,
                    style = MaterialTheme.typography.labelMedium,
                    color = IrisColors.MutedForeground,
                )
            }
            Text(
                channel?.name ?: fallbackName,
                style = MaterialTheme.typography.titleMedium,
                color = IrisColors.Foreground,
            )
        }
        if (now != null) {
            Text(
                now.title,
                style = MaterialTheme.typography.bodyMedium,
                color = IrisColors.MutedForeground,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Box(Modifier.fillMaxWidth(0.4f)) {
                ProgrammeProgressBar(
                    startEpochMs = now.start.toInstant().toEpochMilli(),
                    stopEpochMs = now.stop.toInstant().toEpochMilli(),
                )
            }
        }
        val next = nowNext?.next
        if (next != null) {
            Text(
                "Up next · ${next.title}",
                style = MaterialTheme.typography.bodySmall,
                color = IrisColors.FgDim,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}
