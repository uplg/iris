// File-level opt-in: PlayerView / MediaItem / buildPlayer are all
// @UnstableApi; `@OptIn` doesn't propagate into AndroidView lambdas.
@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package studio.kahn.iris.tv.ui.screens

import android.view.KeyEvent
import androidx.compose.foundation.background
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
    var autoRetried by remember(channelId) { mutableStateOf(false) }
    // Bumped by the Retry button to force a re-prepare of the same channel.
    var retryNonce by remember { mutableStateOf(0) }

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

    // Error listener: report the failure (backend cools the source down and
    // elects the next feed), then one silent re-prepare — the reload fetches
    // a fresh master, i.e. the newly elected source. Second failure shows
    // the error UI.
    DisposableEffect(player.value) {
        val p = player.value ?: return@DisposableEffect onDispose {}
        val listener = object : Player.Listener {
            override fun onPlayerError(error: PlaybackException) {
                serverUrl?.let { url ->
                    container.applicationScope.launch {
                        runCatching {
                            container.apiFor(url).liveTvPlaybackError(country, channelId)
                        }
                    }
                }
                if (!autoRetried) {
                    autoRetried = true
                    p.prepare()
                    p.playWhenReady = true
                } else {
                    errorMessage = humanizePlaybackError(error).first
                }
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

    // Held so the key handler can tell whether the controller overlay is up —
    // DPAD up/down must keep navigating the controller when it's visible and
    // only zap channels when it's hidden. CHANNEL_UP/DOWN always zap.
    var playerViewRef by remember { mutableStateOf<PlayerView?>(null) }

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
                val controllerUp = playerViewRef?.isControllerFullyVisible == true
                when (event.nativeKeyEvent.keyCode) {
                    KeyEvent.KEYCODE_CHANNEL_UP -> {
                        zap(-1)
                        true
                    }
                    KeyEvent.KEYCODE_CHANNEL_DOWN -> {
                        zap(1)
                        true
                    }
                    KeyEvent.KEYCODE_DPAD_UP -> {
                        if (controllerUp) false else {
                            zap(-1)
                            true
                        }
                    }
                    KeyEvent.KEYCODE_DPAD_DOWN -> {
                        if (controllerUp) false else {
                            zap(1)
                            true
                        }
                    }
                    else -> false
                }
            },
    ) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { ctx ->
                PlayerView(ctx).apply {
                    this.player = player.value
                    useController = true
                    controllerAutoShow = true
                    // Live stream: no timeline scrubbing / episode chrome.
                    setShowFastForwardButton(false)
                    setShowRewindButton(false)
                    setShowNextButton(false)
                    setShowPreviousButton(false)
                    setShowSubtitleButton(false)
                    keepScreenOn = true
                    layoutParams = android.widget.FrameLayout.LayoutParams(
                        android.view.ViewGroup.LayoutParams.MATCH_PARENT,
                        android.view.ViewGroup.LayoutParams.MATCH_PARENT,
                    )
                    playerViewRef = this
                }
            },
            update = { it.player = player.value },
        )

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
                        IrisButton("Retry", onClick = {
                            autoRetried = false
                            retryNonce++
                        })
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
