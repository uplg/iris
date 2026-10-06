// buildPlayer / MediaItem are `@UnstableApi` (androidx RequiresOptIn).
@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package studio.kahn.iris.tv.ui.screens

import android.content.Context
import android.content.pm.PackageManager
import android.text.format.DateFormat
import android.view.KeyEvent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.edit
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.datasource.HttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaSession
import androidx.tv.material3.Text
import java.time.OffsetDateTime
import java.util.Date
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveNowNext
import studio.kahn.iris.tv.data.buildMediaItem
import studio.kahn.iris.tv.data.serverBase
import studio.kahn.iris.tv.data.buildPlayer
import studio.kahn.iris.tv.data.humanizePlaybackError
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.KeyHints
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.LockLandscape
import studio.kahn.iris.tv.ui.components.Meter
import studio.kahn.iris.tv.ui.components.OnOutputLost
import studio.kahn.iris.tv.ui.components.PlayerStage
import studio.kahn.iris.tv.ui.components.Spinner
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.buildMediaSession
import studio.kahn.iris.tv.ui.screens.live.ENCRYPTED_WORDS
import studio.kahn.iris.tv.ui.screens.live.LiveErrorStep
import studio.kahn.iris.tv.ui.screens.live.LiveWatchViewModel
import studio.kahn.iris.tv.ui.screens.live.RETRY_BUDGET_REFILL_MS
import studio.kahn.iris.tv.ui.screens.live.liveErrorStep
import studio.kahn.iris.tv.ui.screens.live.nextWords
import studio.kahn.iris.tv.ui.screens.live.nowWords
import studio.kahn.iris.tv.ui.screens.live.programmeProgress
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType
import studio.kahn.iris.tv.ui.state.RepeatWhileStarted
import studio.kahn.iris.tv.ui.state.irisViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle

/** How long the channel strip stays up after it (re)appears. */
private const val OVERLAY_VISIBLE_MS = 4_000L

/** Automatic reconnect attempts on a playback error before giving up to the
 *  Retry UI. Each attempt first tells the backend to demote the dead source,
 *  then reloads, so the retries walk through a channel's fallback feeds.
 *
 *  Must therefore EXCEED the largest realistic fallback count, or the last
 *  feeds are unreachable: M6 has no official CDN left and is carried by four
 *  Vavoo feeds alone, which a budget of 3 could never walk to the end of. */
private const val MAX_AUTO_RETRIES = 6

/** Decode escalation ladder for a silent no-start. ExoPlayer can sit in
 *  BUFFERING forever WITHOUT raising `onPlayerError` (hardware decoders wedge
 *  on interlaced/corrupt H.264 restreams).
 *
 *  HARDWARE → [HW_STALL_MS] → SOFTWARE (same source) → [SW_STALL_MS] →
 *  SERVER (the backend deinterlaces + re-encodes; M6's only living feed
 *  defeats BOTH local decoders) → [SRV_STALL_MS] → error card.
 *
 *  Advancing a stage never demotes a source (a stall is a local decode
 *  problem): a false positive only costs efficiency. A wrongly-demoted
 *  source was the historical regression. */
private enum class DecodeStage { Hardware, Software, Server }

private const val HW_STALL_MS = 12_000L
private const val SW_STALL_MS = 18_000L

/** ffmpeg needs to probe the live input + emit its first segments. */
private const val SRV_STALL_MS = 40_000L

/** Persisted per-channel decode stage, so a channel that needed the ladder
 *  starts at its working stage on later opens, across restarts. Entries
 *  expire after [STAGE_TTL_MS]: these restreams vary with programming. */
private const val STAGE_PREFS = "livetv_decode_stage"
private const val STAGE_TTL_MS = 24L * 60 * 60 * 1_000

private fun recallStage(context: Context, key: String): DecodeStage {
    val raw = context.getSharedPreferences(STAGE_PREFS, Context.MODE_PRIVATE)
        .getString(key, null) ?: return DecodeStage.Hardware
    val (name, ts) = raw.split(':', limit = 2).let {
        (it.getOrNull(0) ?: "") to (it.getOrNull(1)?.toLongOrNull() ?: 0L)
    }
    if (System.currentTimeMillis() - ts > STAGE_TTL_MS) return DecodeStage.Hardware
    return runCatching { DecodeStage.valueOf(name) }.getOrDefault(DecodeStage.Hardware)
}

private fun persistStage(context: Context, key: String, stage: DecodeStage) {
    context.getSharedPreferences(STAGE_PREFS, Context.MODE_PRIVATE).edit {
        putString(key, "${stage.name}:${System.currentTimeMillis()}")
    }
}

/**
 * Live channel playback. Deliberately NOT [WatchScreen] (torrent-coupled):
 * Live TV is a plain HLS stream from the backend proxy; [buildMediaItem]
 * routes the `.m3u8` to `HlsMediaSource` and the media OkHttp client brings
 * cookie auth and the transparent 401 refresh.
 *
 * ↑/↓ (and the channel keys) zap through the country's list; OK opens the
 * actions (Try another source, Channels); Back closes them, then leaves. On a
 * playback error the backend demotes the source and the screen reloads, up
 * to [MAX_AUTO_RETRIES]; a silent stall walks the decode ladder.
 */
@Composable
fun LiveTvWatchScreen(
    container: AppContainer,
    country: String,
    initialChannelId: String,
    onBack: () -> Unit,
) {
    LockLandscape()
    val context = LocalContext.current
    val vm = irisViewModel(container) { c, saved -> LiveWatchViewModel(c, country, initialChannelId, saved) }
    val serverUrl by vm.baseUrl.collectAsStateWithLifecycle()
    val channelsRead by vm.channels.collectAsStateWithLifecycle()
    val channels = channelsRead.valueOrNull.orEmpty()
    val channelId by vm.channelId.collectAsStateWithLifecycle()
    val guide by vm.guide.collectAsStateWithLifecycle()
    var errorMessage by remember { mutableStateOf<String?>(null) }
    // Channels the master refused as DRM-locked on this visit, before the list says so.
    var lockedHere by remember { mutableStateOf(emptySet<String>()) }
    val encrypted = channels.firstOrNull { it.id == channelId }?.encrypted == true || channelId in lockedHere
    // STABLE state (not re-keyed) + reset per channel below: a long-lived
    // Player.Listener would otherwise capture a stale re-keyed state object
    // after a zap and count against the wrong channel.
    var autoRetryCount by remember { mutableIntStateOf(0) }
    // Bumped to re-prepare the current channel (Retry, the post-demotion reconnect).
    var retryNonce by remember { mutableIntStateOf(0) }
    // Has the current attempt started playing? Drives "Connecting…" and the stall ladder.
    var playing by remember { mutableStateOf(false) }
    // Picture gone while the activity stayed started (`OnOutputLost`): the
    // next remote key reloads at the live edge.
    var outputLost by remember { mutableStateOf(false) }
    var actionsShown by remember { mutableStateOf(false) }
    var swallowCentreUp by remember { mutableStateOf(false) }
    // Compose owns focus + input here (the stage is pure display).
    val rootFocus = remember { FocusRequester() }
    val retryFocus = remember { FocusRequester() }
    val actionsFocus = remember { FocusRequester() }

    var overlayVisible by remember { mutableStateOf(true) }
    var overlayTick by remember { mutableIntStateOf(0) }
    LaunchedEffect(channelId, overlayTick) {
        overlayVisible = true
        delay(OVERLAY_VISIBLE_MS)
        overlayVisible = false
    }

    val player = remember { mutableStateOf<ExoPlayer?>(null) }
    val session = remember { mutableStateOf<MediaSession?>(null) }

    // Primed from what the channel needed before.
    var stage by remember { mutableStateOf(recallStage(context, "$country:${vm.channelId.value}")) }
    // The stage the CURRENT player was built for: renderers are fixed at construction.
    val playerStage = remember { mutableStateOf(DecodeStage.Hardware) }

    // A new channel: a fresh retry budget and its own stage, both set with the channel so the
    // load below runs once, at the right stage.
    val watch: (String) -> Unit = { id ->
        autoRetryCount = 0
        stage = recallStage(context, "$country:$id")
        vm.watch(id)
    }
    LaunchedEffect(playing) {
        if (!playing) return@LaunchedEffect
        persistStage(context, "$country:$channelId", playerStage.value)
        // Glitches hours apart are not one failing feed: a feed that plays a while refills it.
        delay(RETRY_BUDGET_REFILL_MS)
        autoRetryCount = 0
    }

    // Shared failure path (error listener + connect timeout): demote the dead
    // source, WAIT for that POST, then reload the newly elected feed. Past the
    // budget, the Retry card.
    val onFail: (String) -> Unit = { message ->
        val failed = channelId
        if (autoRetryCount < MAX_AUTO_RETRIES) {
            autoRetryCount++
            container.applicationScope.launch {
                vm.reportFailure(failed)
                // A zap meanwhile: the new channel is not reloaded for the old one's failure.
                if (channelId == failed) retryNonce++
            }
        } else {
            container.applicationScope.launch { vm.reportFailure(failed) }
            errorMessage = message
        }
    }

    // "Try another source" (the web's escape hatch for a feed that plays
    // badly): report it, then start again on the next one with a fresh budget.
    val anotherSource: () -> Unit = {
        val failed = channelId
        actionsShown = false
        autoRetryCount = 0
        errorMessage = null
        container.applicationScope.launch {
            vm.reportFailure(failed)
            if (channelId == failed) retryNonce++
        }
    }

    // (Re)load on a channel change, Retry, or a stage advance.
    LaunchedEffect(channelId, serverUrl, retryNonce, stage, encrypted) {
        val url = serverUrl ?: return@LaunchedEffect
        errorMessage = null
        playing = false
        outputLost = false
        if (encrypted) {
            player.value?.stop()
            return@LaunchedEffect
        }
        val base = serverBase(url)
        val masterUrl = if (stage == DecodeStage.Server) {
            "${base}api/livetv/$country/channels/$channelId/transcode/master.m3u8"
        } else {
            "${base}api/livetv/$country/channels/$channelId/master.m3u8"
        }
        val name = channels.firstOrNull { it.id == channelId }?.name ?: channelId
        if (player.value != null && playerStage.value != stage) {
            session.value?.release()
            session.value = null
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
            session.value = buildMediaSession(context, it, "live")
        }
        p.setMediaItem(buildMediaItem(masterUrl, name))
        p.prepare()
        p.playWhenReady = true
    }

    // Stall escape hatch: re-armed per attempt, counted only while the screen is started and
    // the picture reaches someone (a stop is not a stall: it would persist a stage for 24 h).
    // A silent no-start walks the ladder; it never demotes the source nor burns the retry walk.
    RepeatWhileStarted(listOf(channelId, retryNonce, serverUrl, stage, outputLost, encrypted)) {
        if (outputLost || encrypted) return@RepeatWhileStarted
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

    // "Connecting…" clears on ANY sign of life (first frame, isPlaying,
    // STATE_READY), synced right after attaching: a fast channel can start
    // before this effect runs.
    DisposableEffect(player.value) {
        val p = player.value ?: return@DisposableEffect onDispose {}
        val listener = object : Player.Listener {
            override fun onPlayerError(error: PlaybackException) {
                when (liveErrorStep(error.errorCode, isEncryptedRefusal(error))) {
                    LiveErrorStep.Locked -> lockedHere = lockedHere + channelId
                    LiveErrorStep.Rejoin -> {
                        p.seekToDefaultPosition()
                        p.prepare()
                    }
                    LiveErrorStep.Rotate -> onFail(humanizePlaybackError(error).first)
                }
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
            session.value?.release()
            session.value = null
            player.value?.release()
            player.value = null
        }
    }

    // Home (`ON_STOP`) cuts the stream; unlike VOD, a live channel resumes
    // by itself on return (`ON_START`), at the live edge.
    val lifecycleOwner = LocalLifecycleOwner.current
    val stoppedByLifecycle = remember { mutableStateOf(false) }
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_STOP -> {
                    if (player.value != null) {
                        stoppedByLifecycle.value = true
                        player.value?.stop()
                    }
                }
                Lifecycle.Event.ON_START -> {
                    if (stoppedByLifecycle.value) {
                        stoppedByLifecycle.value = false
                        retryNonce++
                    }
                }
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }
    OnOutputLost {
        if (player.value != null && !outputLost) {
            outputLost = true
            player.value?.stop()
        }
    }

    val zap: (Int) -> Unit = { delta ->
        vm.channelAt(delta)?.let(watch)
        actionsShown = false
    }

    LaunchedEffect(errorMessage, actionsShown, encrypted) {
        runCatching {
            when {
                errorMessage != null || encrypted -> retryFocus.requestFocus()
                actionsShown -> actionsFocus.requestFocus()
                else -> rootFocus.requestFocus()
            }
        }
    }
    BackHandler(enabled = actionsShown && errorMessage == null) { actionsShown = false }

    val touchscreen = remember(context) { context.packageManager.hasSystemFeature(PackageManager.FEATURE_TOUCHSCREEN) }
    val format = remember(context) { DateFormat.getTimeFormat(context) }
    val clock = remember(format) { { t: OffsetDateTime -> format.format(Date.from(t.toInstant())) } }

    Box(
        Modifier
            .fillMaxSize()
            .background(IrisColor.stage)
            .onPreviewKeyEvent { event ->
                val code = event.nativeKeyEvent.keyCode
                val centre = code == KeyEvent.KEYCODE_DPAD_CENTER || code == KeyEvent.KEYCODE_ENTER ||
                    code == KeyEvent.KEYCODE_NUMPAD_ENTER
                if (event.nativeKeyEvent.action != KeyEvent.ACTION_DOWN) {
                    // The press that opened the actions must not click the button it focused.
                    if (centre && swallowCentreUp) {
                        swallowCentreUp = false
                        return@onPreviewKeyEvent true
                    }
                    return@onPreviewKeyEvent false
                }
                overlayTick++
                // First key once the picture is back: rejoin the live edge. Back still leaves.
                if (outputLost && event.nativeKeyEvent.keyCode != KeyEvent.KEYCODE_BACK) {
                    retryNonce++
                    return@onPreviewKeyEvent true
                }
                // Up/down zap even while the error card's or the actions' buttons
                // hold focus; left/right/centre fall through to them.
                when (event.nativeKeyEvent.keyCode) {
                    KeyEvent.KEYCODE_CHANNEL_UP, KeyEvent.KEYCODE_DPAD_UP -> {
                        zap(-1)
                        true
                    }
                    KeyEvent.KEYCODE_CHANNEL_DOWN, KeyEvent.KEYCODE_DPAD_DOWN -> {
                        zap(1)
                        true
                    }
                    KeyEvent.KEYCODE_DPAD_CENTER, KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> {
                        when {
                            swallowCentreUp -> true
                            !actionsShown && errorMessage == null && !encrypted -> {
                                actionsShown = true
                                swallowCentreUp = true
                                true
                            }
                            else -> false
                        }
                    }
                    else -> false
                }
            }
            .focusRequester(rootFocus)
            .focusable()
            .pointerInput(Unit) {
                detectTapGestures {
                    overlayTick++
                    if (errorMessage == null && !encrypted) actionsShown = !actionsShown
                }
            },
    ) {
        PlayerStage(player.value, liftCues = overlayVisible || actionsShown)

        val channel = channels.firstOrNull { it.id == channelId }
        val nowNext = guide.entries[channelId]
        if ((overlayVisible || actionsShown) && !outputLost) {
            LiveTopBar(
                channel = channel,
                fallbackName = channelId,
                nowNext = nowNext,
                clock = clock,
                modifier = Modifier.align(Alignment.TopStart),
            )
            LiveBottomBar(
                nowNext = nowNext,
                nowMs = guide.readAtMs,
                clock = clock,
                actionsShown = actionsShown,
                actionsFocus = actionsFocus,
                onAnotherSource = anotherSource,
                onChannels = onBack,
                onZap = zap.takeIf { touchscreen },
                listError = channelsRead.errorOrNull?.message,
                modifier = Modifier.align(Alignment.BottomStart),
            )
        }

        if (outputLost) {
            CenterNote(
                title = "The picture went away, so the live stream was stopped.",
                detail = "Press any button to rejoin the live edge.",
            )
        }

        if (errorMessage == null && !playing && !outputLost && !encrypted) {
            ConnectingNote(attempt = listOf(channelId, retryNonce, stage), stage = stage, autoRetryCount = autoRetryCount)
        }

        val error = errorMessage
        if (encrypted) {
            LiveErrorCard(
                title = "This channel can't be played",
                message = ENCRYPTED_WORDS,
                retryFocus = retryFocus,
                onRetry = null,
                onChannels = onBack,
            )
        } else if (error != null) {
            LiveErrorCard(
                title = "Stream unavailable",
                message = error,
                retryFocus = retryFocus,
                onRetry = {
                    autoRetryCount = 0
                    errorMessage = null
                    retryNonce++
                },
                onChannels = onBack,
            )
        }
    }
}

/** The master answered 409: every feed of the channel is DRM-locked with no licence Iris can obtain. */
private fun isEncryptedRefusal(error: PlaybackException): Boolean =
    generateSequence<Throwable>(error) { it.cause }
        .any { it is HttpDataSource.InvalidResponseCodeException && it.responseCode == 409 }

/** The channel strip (the player's top bar, live): number and name, what is on now. */
@Composable
internal fun LiveTopBar(
    channel: LiveChannel?,
    fallbackName: String,
    nowNext: LiveNowNext?,
    clock: (OffsetDateTime) -> String,
    modifier: Modifier = Modifier,
) {
    val layout = IrisLayout.current
    Row(
        modifier
            .fillMaxWidth()
            .background(IrisColor.stageScrim)
            .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, top = layout.safeVertical, bottom = 14.dp),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
    ) {
        val number = channel?.tntNumber
        if (number != null) {
            Text(number.toString(), style = IrisType.stageTitle, color = IrisColor.stageMuted, modifier = Modifier.alignByBaseline())
        }
        Text(
            channel?.name ?: fallbackName,
            style = IrisType.stageTitle,
            color = IrisColor.stageInk,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.alignByBaseline().weight(1f, fill = false),
        )
        nowNext?.now?.let { now ->
            Text(
                nowWords(now, clock),
                style = IrisType.metaLarge,
                color = IrisColor.stageMuted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.alignByBaseline().weight(1f, fill = false),
            )
        }
        if (channel?.geoBlocked == true) {
            Text("May be blocked in your country", style = IrisType.meta, color = IrisColor.stageMuted, modifier = Modifier.alignByBaseline())
        }
    }
}

/** How far into the programme, what is next, the keys; OK adds the actions. */
@Composable
internal fun LiveBottomBar(
    nowNext: LiveNowNext?,
    nowMs: Long,
    clock: (OffsetDateTime) -> String,
    actionsShown: Boolean,
    actionsFocus: FocusRequester,
    onAnotherSource: () -> Unit,
    onChannels: () -> Unit,
    modifier: Modifier = Modifier,
    /** Touch: the previous / next channel as buttons (no ↑/↓ on a phone). */
    onZap: ((Int) -> Unit)? = null,
    /** Why the channel list did not load: the channel can't change until it does. */
    listError: String? = null,
) {
    val layout = IrisLayout.current
    Column(
        modifier
            .fillMaxWidth()
            .background(IrisColor.stageScrim)
            .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, top = 16.dp, bottom = 23.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        val now = nowNext?.now
        val progress = now?.let { programmeProgress(it.start, it.stop, nowMs) }
        if (now != null) {
            Row(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(clock(now.start), style = IrisType.figure, color = IrisColor.stageInk)
                Meter(progress ?: 0f, Modifier.weight(1f), track = IrisColor.stageLine, height = 5.dp)
                Text(clock(now.stop), style = IrisType.figure, color = IrisColor.stageMuted)
            }
        }
        nowNext?.next?.let { next ->
            Text(nextWords(next, clock), style = IrisType.meta, color = IrisColor.stageMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (listError != null) {
            StatusLine("The channel list did not load, so the channel can't change: $listError", tone = StatusTone.Down)
        }
        if (actionsShown) {
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
                ActionButton(
                    "Try another source",
                    onAnotherSource,
                    style = ActionStyle.Quiet,
                    icon = Icons.Rounded.Refresh,
                    modifier = Modifier.focusRequester(actionsFocus),
                )
                ActionButton("Channels", onChannels, style = ActionStyle.Quiet)
                if (onZap != null) {
                    ActionButton("Previous channel", { onZap(-1) }, style = ActionStyle.Quiet)
                    ActionButton("Next channel", { onZap(1) }, style = ActionStyle.Quiet)
                }
            }
        }
        KeyHints(
            listOf(
                KeyHint("${Keys.UP} ${Keys.DOWN}", "Change channel"),
                if (actionsShown) KeyHint(Keys.BACK, "Hide these buttons") else KeyHint(Keys.OK, "Another source, channels"),
                if (actionsShown) KeyHint(Keys.OK, "Choose") else KeyHint(Keys.BACK, "Channels"),
            ),
            onStage = true,
        )
    }
}

/**
 * "Connecting…" with the seconds since [attempt] began, ticking here only (the screen does not
 * recompose each second). Diagnostic by design: no adb on the household TVs, so the stage and
 * the elapsed seconds are the debugging story.
 */
@Composable
private fun ConnectingNote(attempt: Any, stage: DecodeStage, autoRetryCount: Int) {
    var elapsedS by remember(attempt) { mutableIntStateOf(0) }
    LaunchedEffect(attempt) {
        while (true) {
            delay(1_000)
            elapsedS++
        }
    }
    val title = when (stage) {
        DecodeStage.Hardware -> "Connecting…"
        DecodeStage.Software -> "Slow start, retrying with the software decoder…"
        DecodeStage.Server -> "Preparing a compatible stream on the server…"
    }
    val detail = buildString {
        append("${elapsedS}s")
        if (autoRetryCount > 0) append(" · source attempt ${autoRetryCount + 1}")
        append(
            when (stage) {
                DecodeStage.Hardware -> " · hw"
                DecodeStage.Software -> " · sw"
                DecodeStage.Server -> " · srv"
            },
        )
    }
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
            Spinner(Modifier.size(22.dp), color = IrisColor.accent)
            Text(title, style = IrisType.body, color = IrisColor.stageInk, textAlign = TextAlign.Center)
            Text(detail, style = IrisType.meta, color = IrisColor.stageMuted)
        }
    }
}

@Composable
private fun CenterNote(title: String, detail: String) {
    Box(Modifier.fillMaxSize().background(IrisColor.stage), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
            Text(title, style = IrisType.body, color = IrisColor.stageInk, textAlign = TextAlign.Center)
            Text(detail, style = IrisType.meta, color = IrisColor.stageMuted, textAlign = TextAlign.Center)
        }
    }
}

@Composable
private fun LiveErrorCard(
    title: String,
    message: String,
    retryFocus: FocusRequester,
    /** Null when trying again can't help: the focus goes to the way back. */
    onRetry: (() -> Unit)?,
    onChannels: () -> Unit,
) {
    Box(Modifier.fillMaxSize().background(IrisColor.overlay), contentAlignment = Alignment.Center) {
        Column(
            Modifier
                .widthIn(max = 460.dp)
                .padding(horizontal = IrisSpace.s8)
                .background(IrisColor.stageScrim, IrisShape.panel)
                .padding(IrisSpace.s8),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s4),
        ) {
            StatusLine(title, tone = StatusTone.Down, style = IrisType.bodyStrong)
            Text(message, style = IrisType.body, color = IrisColor.stageMuted)
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
                if (onRetry != null) {
                    ActionButton("Retry", onRetry, icon = Icons.Rounded.Refresh, modifier = Modifier.focusRequester(retryFocus))
                    ActionButton("Back to channels", onChannels, style = ActionStyle.Secondary)
                } else {
                    ActionButton("Back to channels", onChannels, modifier = Modifier.focusRequester(retryFocus))
                }
            }
        }
    }
}
