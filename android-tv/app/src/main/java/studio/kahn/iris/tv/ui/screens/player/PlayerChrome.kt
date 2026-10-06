@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package studio.kahn.iris.tv.ui.screens.player

import android.text.format.DateFormat
import android.view.KeyEvent
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.Player
import androidx.media3.common.util.Util
import androidx.media3.ui.compose.state.rememberPlayPauseButtonState
import androidx.media3.ui.compose.state.rememberProgressStateWithTickInterval
import androidx.tv.material3.Text
import java.util.Date
import kotlinx.coroutines.delay
import studio.kahn.iris.tv.ui.components.ConfirmDialog
import studio.kahn.iris.tv.ui.components.PlayerKeyRouter
import studio.kahn.iris.tv.ui.components.Spinner
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType
import studio.kahn.iris.tv.ui.format.episodeCode

/** How much of the chrome shows. */
enum class ChromeMode {
    Hidden,
    /** Shown, focus stays on the picture: ←/→ seek, OK plays or pauses, it hides by itself while playing. */
    Peek,
    /** Focus is in the buttons: it stays until Back or ↑. */
    Engaged,
}

enum class PlayerPanel { None, Tracks, Episodes }

/** The chrome's interaction state, separate from the player's. */
@Stable
class ChromeState {
    var mode by mutableStateOf(ChromeMode.Hidden)
    var panel by mutableStateOf(PlayerPanel.None)
    /** A seek being chosen with a held ←/→ or a drag. */
    var previewMs by mutableStateOf<Long?>(null)
    /** Bumped on every interaction: restarts the auto-hide countdown. */
    var touched by mutableIntStateOf(0)
    /** Engage on Next rather than on play/pause (the end of an episode). */
    var engageOnNext by mutableStateOf(false)
    var seekOriginMs = 0L
    var seekForward = true

    fun peek() {
        if (mode == ChromeMode.Hidden) mode = ChromeMode.Peek
        touched++
    }
}

private const val AUTO_HIDE_MS = 4_000L

/**
 * The player's chrome over [PlayerStage]: the controls, the side panels, the
 * "prepare it?" prompt and an error notice, and the remote's keys.
 *
 * Keys: while the buttons don't hold the focus (Hidden, Peek), the activity
 * hands every key here first ([PlayerKeyRouter]): ←/→ seek 10 s (held: faster,
 * committed on release), OK plays or pauses, ↓ moves into the buttons, ↑
 * shows the bar, Back hides (then leaves). Media keys work in every mode; the
 * MediaSession takes them when the app is not in front. Auto-hide only in
 * Peek while playing, never with the focus in the buttons or a panel open.
 * Touch: a tap on the picture shows or hides, the buttons tap, the bar drags.
 */
@Composable
fun PlayerChrome(
    vm: WatchViewModel,
    playback: VodPlayback,
    chrome: ChromeState,
    header: WatchHeader,
    forSeries: Boolean,
    onBack: () -> Unit,
    onNavigateToFile: (String, Int) -> Unit,
) {
    val player = playback.player ?: return
    val buttonFocus = remember { PlayerButtonFocus() }
    val rootFocus = remember { FocusRequester() }
    val playPause = rememberPlayPauseButtonState(player)
    val episodeContext by vm.episodeContext.collectAsStateWithLifecycle()
    val notice by vm.notice.collectAsStateWithLifecycle()
    val busy by vm.busy.collectAsStateWithLifecycle()
    val sideTitle by vm.sideTitle.collectAsStateWithLifecycle()
    val probe = vm.setup.collectAsStateWithLifecycle().value?.probe

    val next = episodeContext?.next
    val nextOnDisk = remember(next) { NextEpisodeRule.onDisk(next) }
    val offersNext = NextEpisodeRule.offersNext(next, header.isMovie, playback.nearEnd, playback.ended)
    val tracks = playback.tracks
    val route = playback.route
    val menu = remember(tracks, probe, route) { trackMenu(tracks, probe, route) }
    var prepareAsked by remember { mutableStateOf(false) }
    var prepareOpen by remember { mutableStateOf(false) }
    LaunchedEffect(playback.nearEnd, playback.ended, episodeContext) {
        if (NextEpisodeRule.promptsPrepare(next, episodeContext?.followed == true, playback.nearEnd, playback.ended, prepareAsked)) {
            prepareAsked = true
            prepareOpen = true
        }
    }

    val errorShown = playback.error != null
    val routerActive = chrome.mode != ChromeMode.Engaged && chrome.panel == PlayerPanel.None && !prepareOpen && !errorShown

    val goNext: () -> Unit = { nextOnDisk?.let { onNavigateToFile(it.infohash, it.fileIdx) } }
    val togglePlay: () -> Unit = { Util.handlePlayPauseButtonAction(player) }

    fun engage(onNext: Boolean = false) {
        chrome.mode = ChromeMode.Engaged
        chrome.touched++
        chrome.previewMs = null
        chrome.engageOnNext = onNext
    }

    fun disengage() {
        chrome.mode = ChromeMode.Peek
        chrome.touched++
        runCatching { rootFocus.requestFocus() }
    }

    // The end with nowhere to go but Next or Back: the buttons, on Next.
    LaunchedEffect(playback.ended) {
        if (playback.ended && chrome.panel == PlayerPanel.None) engage(onNext = offersNext)
    }
    LaunchedEffect(chrome.mode, chrome.engageOnNext) {
        if (chrome.mode == ChromeMode.Engaged) {
            val target = if (chrome.engageOnNext) buttonFocus.next else buttonFocus.play
            if (runCatching { target.requestFocus() }.isFailure) runCatching { buttonFocus.play.requestFocus() }
        }
    }
    LaunchedEffect(chrome.mode, chrome.panel, chrome.touched, playPause.showPlay) {
        if (chrome.mode == ChromeMode.Peek && chrome.panel == PlayerPanel.None && !playPause.showPlay) {
            delay(AUTO_HIDE_MS)
            chrome.mode = ChromeMode.Hidden
        }
    }
    LaunchedEffect(Unit) { runCatching { rootFocus.requestFocus() } }

    val routerOn by rememberUpdatedState(routerActive)
    val durationOf: () -> Long = {
        playback.filmDurationMs.takeIf { it > 0 } ?: player.duration.coerceAtLeast(0)
    }
    DisposableEffect(player) {
        val mine: (KeyEvent) -> Boolean = handler@{ event ->
            if (handleMediaKey(player, event)) {
                chrome.peek()
                return@handler true
            }
            if (!routerOn) return@handler false
            onRemoteKey(event, chrome, player, durationOf(), togglePlay) { engage() }
        }
        PlayerKeyRouter.handler = mine
        // The next episode's screen composes before this one leaves: never clear its handler.
        onDispose { if (PlayerKeyRouter.handler === mine) PlayerKeyRouter.handler = null }
    }
    // Back from Home: the paused frame says where it is (the bar, "Play").
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_START) chrome.peek() }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }

    BackHandler(enabled = chrome.mode != ChromeMode.Hidden && chrome.panel == PlayerPanel.None) {
        chrome.mode = ChromeMode.Hidden
        chrome.previewMs = null
        runCatching { rootFocus.requestFocus() }
    }

    val shown = chrome.mode != ChromeMode.Hidden || chrome.panel != PlayerPanel.None
    Box(
        Modifier
            .fillMaxSize()
            .focusRequester(rootFocus)
            .focusable()
            .pointerInput(Unit) {
                detectTapGestures {
                    if (chrome.mode == ChromeMode.Hidden) chrome.peek() else chrome.mode = ChromeMode.Hidden
                }
            },
    ) {
        BufferingMark(player, Modifier.align(Alignment.Center))
        AnimatedVisibility(visible = shown && chrome.panel == PlayerPanel.None, enter = fadeIn(), exit = fadeOut()) {
            val progress = rememberProgressStateWithTickInterval(player, tickIntervalMs = 1_000)
            val facts by vm.facts.collectAsStateWithLifecycle()
            val clock by produceClock()
            val sideRows by vm.sideRows.collectAsStateWithLifecycle()
            val sideLabel = sideTitle.takeIf { sideRows.size > 1 }
            PlayerControls(
                title = PlayerTitle(header.title, header.episode, factsWithRoute(facts, playback.route)),
                clock = clock,
                scrub = ScrubPosition(
                    positionMs = progress.currentPositionMs,
                    bufferedMs = progress.bufferedPositionMs,
                    durationMs = durationOf(),
                    previewMs = chrome.previewMs,
                ),
                buttons = PlayerButtons(
                    playing = !playPause.showPlay,
                    showTracks = !menu.isEmpty,
                    sideLabel = sideLabel,
                    nextLabel = nextOnDisk?.label?.takeIf { offersNext },
                    trailing = notice ?: endWords(playback.ended, header.isMovie, offersNext) ?: menu.summary,
                ),
                focus = buttonFocus,
                onPlayPause = {
                    togglePlay()
                    chrome.touched++
                },
                onTracks = { chrome.panel = PlayerPanel.Tracks },
                onSide = {
                    vm.refreshProgress()
                    chrome.panel = PlayerPanel.Episodes
                },
                onNext = goNext,
                onLeaveButtons = ::disengage,
                onPreview = { chrome.previewMs = it; chrome.touched++ },
                onSeek = { ms ->
                    player.seekTo(ms)
                    chrome.previewMs = null
                    chrome.touched++
                },
            )
        }
        val said = notice
        if (said != null && !shown) NoticePill(said, Modifier.align(Alignment.TopEnd))

        when (chrome.panel) {
            PlayerPanel.Tracks -> TracksPanel(
                menu = menu,
                forSeries = forSeries,
                onChoose = { id -> player.choose(playback.tracks, id) },
                onDismiss = { chrome.panel = PlayerPanel.None },
            )
            PlayerPanel.Episodes -> EpisodesPanel(
                title = sideTitle,
                rows = vm.sideRows.collectAsStateWithLifecycle().value,
                busyKey = busy,
                onPlay = { row -> onNavigateToFile(row.infohash, row.fileIdx) },
                onGrab = { row -> vm.grab(row, onNavigateToFile) },
                onDismiss = { chrome.panel = PlayerPanel.None },
            )
            PlayerPanel.None -> Unit
        }
        LaunchedEffect(chrome.panel) {
            if (chrome.panel == PlayerPanel.None && chrome.mode == ChromeMode.Engaged) {
                runCatching { buttonFocus.play.requestFocus() }
            }
        }

        val error = playback.error
        if (error != null) {
            PlayerErrorNotice(error, onRetry = playback::retry, onBack = onBack)
        }
        if (prepareOpen && next != null) {
            ConfirmDialog(
                eyebrow = "Next episode available",
                title = "${episodeCode(next.season, next.episode)} is ready to grab",
                body = "Prepare it for next time? It downloads in the background.",
                confirmLabel = "Prepare",
                onConfirm = {
                    prepareOpen = false
                    vm.prepareNext()
                },
                onCancel = { prepareOpen = false },
            )
        }
    }
}

/** The ending in words, in place of the tracks summary. */
private fun endWords(ended: Boolean, isMovie: Boolean, offersNext: Boolean): String? = when {
    !ended || offersNext -> null
    isMovie -> "You've finished watching."
    else -> "You're all caught up: no more episodes available."
}

private fun factsWithRoute(facts: String, route: PlayRoute): String {
    val how = when (route) {
        PlayRoute.Direct -> null
        PlayRoute.ServerTranscode -> "converted on the server"
        PlayRoute.ServerRemux -> "remuxed on the server"
    }
    return listOfNotNull(facts.ifBlank { null }, how).joinToString(" · ")
}

/** The wall clock for the top bar, in the device's 12/24 h setting, refreshed each minute while shown. */
@Composable
private fun produceClock() = LocalContext.current.let { context ->
    produceState(initialValue = DateFormat.getTimeFormat(context).format(Date())) {
        while (true) {
            value = DateFormat.getTimeFormat(context).format(Date())
            delay(60_000L - System.currentTimeMillis() % 60_000L)
        }
    }
}

/** A spinner while the player waits for data after it started. Its words: the controls' state. */
@Composable
private fun BufferingMark(player: Player, modifier: Modifier) {
    val buffering by produceState(initialValue = false, player) {
        val listener = object : Player.Listener {
            override fun onEvents(player: Player, events: Player.Events) {
                value = player.playbackState == Player.STATE_BUFFERING && player.playWhenReady
            }
        }
        player.addListener(listener)
        value = player.playbackState == Player.STATE_BUFFERING && player.playWhenReady
        try {
            kotlinx.coroutines.awaitCancellation()
        } finally {
            player.removeListener(listener)
        }
    }
    if (buffering) Spinner(modifier.size(28.dp), color = IrisColor.stageInk)
}

@Composable
private fun NoticePill(text: String, modifier: Modifier) {
    val layout = IrisLayout.current
    Text(
        text,
        style = IrisType.meta,
        color = IrisColor.stageInk,
        modifier = modifier
            .padding(top = layout.safeVertical, end = layout.safeHorizontal)
            .widthIn(max = 360.dp)
            .background(IrisColor.stageScrim, IrisShape.pill)
            .padding(horizontal = IrisSpace.s5, vertical = IrisSpace.s3),
    )
}

/** Media keys from the remote, in every mode. */
private fun handleMediaKey(player: Player, event: KeyEvent): Boolean {
    val handled = when (event.keyCode) {
        KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE, KeyEvent.KEYCODE_HEADSETHOOK,
        KeyEvent.KEYCODE_MEDIA_PLAY, KeyEvent.KEYCODE_MEDIA_PAUSE,
        KeyEvent.KEYCODE_MEDIA_FAST_FORWARD, KeyEvent.KEYCODE_MEDIA_REWIND,
        -> true
        else -> false
    }
    if (!handled) return false
    if (event.action != KeyEvent.ACTION_DOWN || event.repeatCount > 0) return true
    when (event.keyCode) {
        KeyEvent.KEYCODE_MEDIA_PLAY -> Util.handlePlayButtonAction(player)
        KeyEvent.KEYCODE_MEDIA_PAUSE -> Util.handlePauseButtonAction(player)
        KeyEvent.KEYCODE_MEDIA_FAST_FORWARD -> player.seekForward()
        KeyEvent.KEYCODE_MEDIA_REWIND -> player.seekBack()
        else -> Util.handlePlayPauseButtonAction(player)
    }
    return true
}

/**
 * A remote key while the buttons don't hold the focus. ←/→: the preview moves
 * from where the press started by [SeekAcceleration] for as long as it is
 * held, and the seek lands on release (one seek, one seek hint).
 */
private fun onRemoteKey(
    event: KeyEvent,
    chrome: ChromeState,
    player: Player,
    durationMs: Long,
    togglePlay: () -> Unit,
    engage: () -> Unit,
): Boolean {
    val down = event.action == KeyEvent.ACTION_DOWN
    return when (event.keyCode) {
        KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT -> {
            val forward = event.keyCode == KeyEvent.KEYCODE_DPAD_RIGHT
            if (down) {
                if (event.repeatCount == 0 || chrome.previewMs == null || chrome.seekForward != forward) {
                    chrome.seekOriginMs = player.currentPosition
                    chrome.seekForward = forward
                }
                chrome.previewMs = SeekAcceleration.target(
                    chrome.seekOriginMs, forward, event.eventTime - event.downTime, durationMs,
                )
                chrome.peek()
            } else {
                chrome.previewMs?.let(player::seekTo)
                chrome.previewMs = null
                chrome.touched++
            }
            true
        }
        KeyEvent.KEYCODE_DPAD_CENTER, KeyEvent.KEYCODE_ENTER,
        KeyEvent.KEYCODE_NUMPAD_ENTER, KeyEvent.KEYCODE_BUTTON_A,
        -> {
            if (down && event.repeatCount == 0) {
                togglePlay()
                chrome.peek()
            }
            true
        }
        KeyEvent.KEYCODE_DPAD_DOWN -> {
            if (down && event.repeatCount == 0) engage()
            true
        }
        KeyEvent.KEYCODE_DPAD_UP -> {
            if (down) chrome.peek()
            true
        }
        KeyEvent.KEYCODE_BACK -> false
        else -> {
            if (down) chrome.peek()
            false
        }
    }
}
