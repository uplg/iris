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
import androidx.compose.foundation.layout.widthIn
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

/** Decode escalation ladder for a silent no-start. ExoPlayer can sit in
 *  BUFFERING forever WITHOUT raising `onPlayerError` (hardware decoders wedge
 *  on interlaced/corrupt H.264 restreams) — without an escape hatch, such a
 *  channel is an eternal "Connecting…" with no retry and no error.
 *
 *  HARDWARE → [HW_STALL_MS] → SOFTWARE (same source) → [SW_STALL_MS] →
 *  SERVER (the backend deinterlaces + re-encodes; verified necessary
 *  on-device: M6's only living feed defeats BOTH local decoders) →
 *  [SRV_STALL_MS] → error card.
 *
 *  The windows are deliberately TIGHT: advancing a stage never demotes a
 *  source (a stall is a local decode problem), so a false positive on a
 *  slow-starting healthy feed only costs efficiency — the next stage plays
 *  it anyway. A wrongly-demoted source was the historical regression; a
 *  wrongly-escalated decode stage is harmless. */
private enum class DecodeStage { Hardware, Software, Server }

private const val HW_STALL_MS = 12_000L
private const val SW_STALL_MS = 18_000L

/** ffmpeg needs to probe the live input + emit its first segments. */
private const val SRV_STALL_MS = 40_000L

/** Persisted per-channel decode stage, so a channel that needed the ladder
 *  starts DIRECTLY at its working stage on later opens — across app
 *  restarts. Entries expire after [STAGE_TTL_MS]: these restreams vary with
 *  programming (interlaced tonight, clean tomorrow), so once a day the
 *  channel gets a fresh shot at plain hardware playback. */
private const val STAGE_PREFS = "livetv_decode_stage"
private const val STAGE_TTL_MS = 24L * 60 * 60 * 1_000

private fun recallStage(context: android.content.Context, key: String): DecodeStage {
    val raw = context.getSharedPreferences(STAGE_PREFS, android.content.Context.MODE_PRIVATE)
        .getString(key, null) ?: return DecodeStage.Hardware
    val (name, ts) = raw.split(':', limit = 2).let {
        (it.getOrNull(0) ?: "") to (it.getOrNull(1)?.toLongOrNull() ?: 0L)
    }
    if (System.currentTimeMillis() - ts > STAGE_TTL_MS) return DecodeStage.Hardware
    return runCatching { DecodeStage.valueOf(name) }.getOrDefault(DecodeStage.Hardware)
}

private fun persistStage(context: android.content.Context, key: String, stage: DecodeStage) {
    context.getSharedPreferences(STAGE_PREFS, android.content.Context.MODE_PRIVATE)
        .edit()
        .putString(key, "${stage.name}:${System.currentTimeMillis()}")
        .apply()
}

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

    // Decode escalation stage (see DecodeStage). Advances on silent stalls;
    // primed per channel from what worked earlier this session.
    var stage by remember { mutableStateOf(DecodeStage.Hardware) }
    // Which stage the CURRENT ExoPlayer instance was built for — a stage
    // switch needs a rebuild (renderers are fixed at construction).
    val playerStage = remember { mutableStateOf(DecodeStage.Hardware) }

    // New channel ⇒ fresh retry budget; stage primed from what this channel
    // needed on previous plays (persisted — skip the doomed stages on a
    // known-bad feed even across app restarts). Keyed on channelId only (NOT
    // retryNonce) so a reconnect doesn't reset the count it's incrementing.
    LaunchedEffect(channelId) {
        autoRetryCount = 0
        stage = recallStage(context, "$country:$channelId")
    }
    // Once an attempt actually plays, remember which stage did it.
    LaunchedEffect(playing) {
        if (playing) persistStage(context, "$country:$channelId", playerStage.value)
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
    // the escalation stage advances.
    LaunchedEffect(channelId, serverUrl, retryNonce, stage) {
        val url = serverUrl ?: return@LaunchedEffect
        errorMessage = null
        playing = false
        val base = if (url.endsWith("/")) url else "$url/"
        // SERVER stage plays the backend's deinterlaced/re-encoded variant;
        // the earlier stages play the plain proxied original.
        val masterUrl = if (stage == DecodeStage.Server) {
            "${base}api/livetv/$country/channels/$channelId/transcode/master.m3u8"
        } else {
            "${base}api/livetv/$country/channels/$channelId/master.m3u8"
        }
        val name = channels.firstOrNull { it.id == channelId }?.name ?: channelId
        // Renderers are fixed at construction — a stage switch needs a fresh
        // ExoPlayer (Server output is clean progressive H.264 → hardware).
        if (player.value != null && playerStage.value != stage) {
            player.value?.release()
            player.value = null
        }
        val p = player.value ?: buildPlayer(
            context,
            container.mediaOkHttpClient,
            preferSoftwareVideo = stage == DecodeStage.Software,
        ).also {
            player.value = it
            playerStage.value = stage
        }
        p.setMediaItem(buildMediaItem(masterUrl, name))
        p.prepare()
        p.playWhenReady = true
    }

    // Stall escape hatch (see DecodeStage): re-armed per attempt, implicitly
    // cancelled by the effect restarting on zap/retry/stage switch. A silent
    // no-start walks the ladder — hardware → software (same source, no
    // demote) → server transcode → error card. A stall is a LOCAL decode
    // problem (web plays these feeds), so unlike onPlayerError it never
    // demotes the source nor burns the retry walk.
    LaunchedEffect(channelId, retryNonce, serverUrl, stage) {
        delay(
            when (stage) {
                DecodeStage.Hardware -> HW_STALL_MS
                DecodeStage.Software -> SW_STALL_MS
                DecodeStage.Server -> SRV_STALL_MS
            },
        )
        if (!playing && errorMessage == null) {
            when (stage) {
                DecodeStage.Hardware -> stage = DecodeStage.Software
                DecodeStage.Software -> stage = DecodeStage.Server
                DecodeStage.Server ->
                    errorMessage = "This feed defeats this device's decoders and the " +
                        "server transcoder. It may be down or badly corrupted."
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

    // Cut the stream when the user leaves via Home (`ON_STOP`) — the same
    // hole we plugged for VOD in WatchScreen: without this, live segments
    // keep downloading in the background indefinitely. Unlike VOD (which
    // deliberately does NOT auto-resume), a live CHANNEL resumes by itself
    // on return (`ON_START`): a paused live stream would fall behind the
    // window anyway, so we reload at the live edge via a `retryNonce` bump,
    // which also re-arms the stall ladder.
    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    val stoppedByLifecycle = remember { mutableStateOf(false) }
    DisposableEffect(lifecycleOwner) {
        val observer = androidx.lifecycle.LifecycleEventObserver { _, event ->
            when (event) {
                androidx.lifecycle.Lifecycle.Event.ON_STOP -> {
                    if (player.value != null) {
                        stoppedByLifecycle.value = true
                        player.value?.stop()
                    }
                }
                androidx.lifecycle.Lifecycle.Event.ON_START -> {
                    if (stoppedByLifecycle.value) {
                        stoppedByLifecycle.value = false
                        retryNonce++
                    }
                }
                else -> {}
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
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

        // Connection overlay until the attempt actually starts playing, so a
        // (re)connect isn't a silent black screen. DIAGNOSTIC by design: no
        // adb on the household TVs, so the stage line + elapsed seconds ARE
        // the debugging story ("which step is it stuck on?"). Cleared by the
        // player listener (or the stall ladder flips to the error card).
        if (errorMessage == null && !playing) {
            // Per-attempt elapsed ticker (Android timers are fine — the
            // no-timer rule is web-only).
            var elapsedS by remember(channelId, retryNonce, stage) {
                mutableStateOf(0)
            }
            LaunchedEffect(channelId, retryNonce, stage) {
                while (true) {
                    delay(1_000)
                    elapsedS++
                }
            }
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        when (stage) {
                            DecodeStage.Hardware -> "Connecting…"
                            DecodeStage.Software -> "Slow start, retrying with the software decoder…"
                            DecodeStage.Server -> "Preparing a compatible stream on the server…"
                        },
                        style = MaterialTheme.typography.bodyMedium,
                        color = IrisColors.MutedForeground,
                    )
                    Text(
                        buildString {
                            append("${elapsedS}s")
                            if (autoRetryCount > 0) append(" · source attempt ${autoRetryCount + 1}")
                            append(
                                when (stage) {
                                    DecodeStage.Hardware -> " · hw"
                                    DecodeStage.Software -> " · sw"
                                    DecodeStage.Server -> " · srv"
                                },
                            )
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = IrisColors.FgDim,
                    )
                }
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
                    // Bounded + centered so a long diagnostic message wraps
                    // instead of overflowing the screen edge.
                    modifier = Modifier
                        .widthIn(max = 560.dp)
                        .padding(horizontal = Spacing.xl),
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
                        textAlign = androidx.compose.ui.text.style.TextAlign.Center,
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
