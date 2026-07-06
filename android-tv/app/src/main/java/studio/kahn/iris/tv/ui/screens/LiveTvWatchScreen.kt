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

/** Silent-stall windows. ExoPlayer can sit in BUFFERING forever WITHOUT
 *  raising `onPlayerError` (hardware decoders wedge on interlaced/corrupt
 *  H.264 restreams) — without an escape hatch, such a channel is an eternal
 *  "Connecting…" with no retry and no error.
 *
 *  Escalation: hardware attempt → [HW_STALL_MS] → software-decoder retry of
 *  the SAME source (no demote — election untouched, so this window can be
 *  tight) → [SW_STALL_MS] → demote / walk alternate sources / Retry card.
 *  The software window stays GENEROUS: a healthy-but-slow restream can
 *  legitimately need >12 s to start, and demoting the one good source before
 *  it ever starts was a real regression. */
private const val HW_STALL_MS = 20_000L
private const val SW_STALL_MS = 45_000L

/** Channels (as `country:id`) that needed software decoding this session —
 *  their next open skips the doomed hardware attempt entirely. */
private val softwareVideoNeeded = java.util.concurrent.ConcurrentHashMap<String, Boolean>()

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
    // Automatic reconnects used up on THIS channel. STABLE state (not re-keyed)
    // + reset via the LaunchedEffect below — a long-lived Player.Listener would
    // otherwise capture a stale re-keyed state object after a zap and count
    // against the wrong channel.
    var autoRetryCount by remember { mutableStateOf(0) }
    // Bumped to force a re-prepare of the current channel (Retry button + the
    // post-demotion auto-reconnect).
    var retryNonce by remember { mutableStateOf(0) }
    // Has the current attempt actually started playing? Drives the "Connecting…"
    // placeholder AND cancels the connect timeout. Stable state + reset in the
    // (re)load effect (same stale-capture reasoning as `autoRetryCount`).
    var playing by remember { mutableStateOf(false) }
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

    // Live-TV rescue mode: after a HARDWARE attempt stalls without ever
    // starting, retry the SAME source with software decoders before touching
    // the source election — interlaced/corrupt H.264 restreams (M6) wedge
    // hardware decoders silently but software copes (see buildPlayer).
    var softwareVideo by remember { mutableStateOf(false) }
    // Which flavor the CURRENT ExoPlayer instance was built with — switching
    // requires a rebuild (renderers are fixed at construction).
    val playerIsSoftware = remember { mutableStateOf(false) }

    // New channel ⇒ fresh retry budget; decoder flavor primed from what this
    // channel needed earlier in the session (skip the doomed hardware attempt
    // on a known-interlaced feed). Keyed on channelId only (NOT retryNonce) so
    // a reconnect doesn't reset the count it's incrementing.
    LaunchedEffect(channelId) {
        autoRetryCount = 0
        softwareVideo = softwareVideoNeeded["$country:$channelId"] == true
    }
    // Once an attempt actually plays, remember which decoder flavor did it.
    LaunchedEffect(playing) {
        if (playing) softwareVideoNeeded["$country:$channelId"] = playerIsSoftware.value
    }

    // Shared failure path, used by BOTH the error listener and the connect
    // timeout: demote the dead source, WAIT for that POST to land, then bump
    // `retryNonce` to reload the newly elected feed. (The old code re-prepared
    // immediately, racing the async demote → same dead source back, why M6
    // never recovered.) After MAX_AUTO_RETRIES, surface the Retry card.
    val onFail: (String) -> Unit = onFail@{ message ->
        val url = serverUrl ?: return@onFail
        if (autoRetryCount < MAX_AUTO_RETRIES) {
            autoRetryCount++
            container.applicationScope.launch {
                runCatching { container.apiFor(url).liveTvPlaybackError(country, channelId) }
                retryNonce++
            }
        } else {
            container.applicationScope.launch {
                runCatching { container.apiFor(url).liveTvPlaybackError(country, channelId) }
            }
            errorMessage = message
        }
    }

    // (Re)load the stream whenever the channel changes, Retry is pressed, or
    // the decoder flavor flips (hardware → software rescue).
    LaunchedEffect(channelId, serverUrl, retryNonce, softwareVideo) {
        val url = serverUrl ?: return@LaunchedEffect
        errorMessage = null
        playing = false
        val base = if (url.endsWith("/")) url else "$url/"
        val masterUrl = "${base}api/livetv/$country/channels/$channelId/master.m3u8"
        val name = channels.firstOrNull { it.id == channelId }?.name ?: channelId
        // Renderers are fixed at construction — a decoder-flavor switch needs
        // a fresh ExoPlayer.
        if (player.value != null && playerIsSoftware.value != softwareVideo) {
            player.value?.release()
            player.value = null
        }
        val p = player.value ?: buildPlayer(
            context,
            container.mediaOkHttpClient,
            preferSoftwareVideo = softwareVideo,
        ).also {
            player.value = it
            playerIsSoftware.value = softwareVideo
        }
        p.setMediaItem(buildMediaItem(masterUrl, name))
        p.prepare()
        p.playWhenReady = true
    }

    // Stall escape hatch (see HW_STALL_MS / SW_STALL_MS): re-armed per
    // attempt, implicitly cancelled by the effect restarting on
    // zap/retry/decoder switch. Escalation on a silent no-start: first retry
    // the SAME source with software decoders (hardware wedges on
    // interlaced/corrupt H.264 — the source itself is fine, web plays it),
    // and only if software stalls too, demote / walk alternatives via onFail.
    LaunchedEffect(channelId, retryNonce, serverUrl, softwareVideo) {
        delay(if (softwareVideo) SW_STALL_MS else HW_STALL_MS)
        if (!playing && errorMessage == null) {
            if (!softwareVideo) {
                softwareVideo = true
            } else {
                onFail("The stream did not start — it may be down or unsupported")
            }
        }
    }

    // Player listener: errors go through the shared failure path; "Connecting…"
    // clears on ANY "we're past connecting" signal — first rendered frame,
    // isPlaying, or reaching STATE_READY. We ALSO sync from the current state
    // right after attaching, because on a fast channel playback can start
    // before this effect runs and we'd otherwise miss the event and hang.
    DisposableEffect(player.value) {
        val p = player.value ?: return@DisposableEffect onDispose {}
        val listener = object : Player.Listener {
            override fun onPlayerError(error: PlaybackException) {
                onFail(humanizePlaybackError(error).first)
            }

            override fun onIsPlayingChanged(isPlaying: Boolean) {
                if (isPlaying) playing = true
            }

            override fun onRenderedFirstFrame() {
                playing = true
            }

            override fun onPlaybackStateChanged(state: Int) {
                if (state == Player.STATE_READY) playing = true
            }
        }
        p.addListener(listener)
        if (p.isPlaying || p.playbackState == Player.STATE_READY) playing = true
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

        // "Connecting…" until the attempt actually starts playing, so a
        // (re)connect isn't a silent black screen. Cleared by onIsPlayingChanged
        // (or the connect timeout, which flips to the error card).
        if (errorMessage == null && !playing) {
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
