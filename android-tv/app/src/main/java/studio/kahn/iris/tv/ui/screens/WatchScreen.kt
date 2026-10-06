@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.SubtitlePick
import studio.kahn.iris.tv.data.isVideoPath
import studio.kahn.iris.tv.ui.components.LockLandscape
import studio.kahn.iris.tv.ui.components.OnOutputLost
import studio.kahn.iris.tv.ui.components.PlayerStage
import studio.kahn.iris.tv.ui.screens.player.ChromeMode
import studio.kahn.iris.tv.ui.screens.player.ChromeState
import studio.kahn.iris.tv.ui.screens.player.GettingReadyContent
import studio.kahn.iris.tv.ui.screens.player.GettingReadyUi
import studio.kahn.iris.tv.ui.screens.player.PlayerChrome
import studio.kahn.iris.tv.ui.screens.player.PlayRoute
import studio.kahn.iris.tv.ui.screens.player.ProbePhase
import studio.kahn.iris.tv.ui.screens.player.RouteTracks
import studio.kahn.iris.tv.ui.screens.player.ReadyInput
import studio.kahn.iris.tv.ui.screens.player.ReadyProblem
import studio.kahn.iris.tv.ui.screens.player.VodEngine
import studio.kahn.iris.tv.ui.screens.player.VodPlayback
import studio.kahn.iris.tv.ui.screens.player.WatchHeader
import studio.kahn.iris.tv.ui.screens.player.WatchSetup
import studio.kahn.iris.tv.ui.screens.player.WatchViewModel
import studio.kahn.iris.tv.ui.screens.player.keptForText
import studio.kahn.iris.tv.ui.screens.player.pictureWords
import studio.kahn.iris.tv.ui.screens.player.playWords
import studio.kahn.iris.tv.ui.screens.player.readiness
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.format.NO_SUBTITLES
import studio.kahn.iris.tv.ui.format.languageName

/**
 * One file, watched: getting ready (each step in words, until the first
 * frame), then the picture with its Compose chrome. The ViewModel owns the
 * reads and polls; [VodEngine] owns the Media3 player; [PlayerChrome] the
 * controls, panels and keys. The 2 s torrent poll reaches only the getting-
 * ready steps and the top bar's facts, never the picture or the controls.
 *
 * [onNavigateToFile] swaps to another file (the next episode, a row of the
 * episodes panel); [onPickAnother] opens a search for another release.
 */
@Composable
fun WatchScreen(
    container: AppContainer,
    infohash: String,
    fileIdx: Int,
    onBack: () -> Unit,
    onNavigateToFile: (String, Int) -> Unit,
    onPickAnother: (String) -> Unit = {},
) {
    LockLandscape()
    val vm = irisViewModel(container, key = "watch:$infohash:$fileIdx") { c, _ -> WatchViewModel(c, infohash, fileIdx) }
    val setup by vm.setup.collectAsStateWithLifecycle()
    val header by vm.header.collectAsStateWithLifecycle()
    val collection by vm.collection.collectAsStateWithLifecycle()
    val playback = remember(vm) { VodPlayback() }
    val chrome = remember(vm) { ChromeState() }
    // From the start, getting ready included: a TV switched off then must not start playing.
    OnOutputLost { playback.outputLost = true }

    Box(Modifier.fillMaxSize().background(IrisColor.stage)) {
        val ready = setup
        if (ready != null) {
            VodEngine(container, vm, ready, header.title, playback)
            PlayerStage(playback.player, liftCues = chrome.mode != ChromeMode.Hidden)
        }
        if (ready != null && playback.firstFrameRendered) {
            PlayerChrome(
                vm = vm,
                playback = playback,
                chrome = chrome,
                header = header,
                keptFor = keptForText(ready.ownLanguages.collectionId, collection?.kind ?: if (header.isMovie) MediaKind.movie else MediaKind.tv),
                onBack = onBack,
                onNavigateToFile = onNavigateToFile,
            )
        } else {
            GettingReadyLayer(vm, playback, header, ready, onBack, onPickAnother)
        }
    }
}

@Composable
private fun GettingReadyLayer(
    vm: WatchViewModel,
    playback: VodPlayback,
    header: WatchHeader,
    setup: WatchSetup?,
    onBack: () -> Unit,
    onPickAnother: (String) -> Unit,
) {
    val torrent by vm.torrent.collectAsStateWithLifecycle()
    val probe by vm.probe.collectAsStateWithLifecycle()
    val busy by vm.busy.collectAsStateWithLifecycle()
    val regrabFailed by vm.regrabError.collectAsStateWithLifecycle()
    val t = torrent.valueOrNull
    val video = setup?.probe?.video?.firstOrNull()
    val base = readiness(
        ReadyInput(
            torrent = t,
            probe = probe,
            playWords = setup?.let { playWords(pictureWords(video?.height, video?.codec, video?.hdr?.value), playback.route) },
            serverPrep = setup != null && playback.serverPrep,
            serverReady = playback.serverReady,
            playStatus = playback.serverStatus,
        ),
    )
    val playerError = playback.error
    val ui = GettingReadyUi(
        title = header.title,
        subtitle = listOfNotNull(header.episode, setup?.let { startLanguages(it, playback.route) }).joinToString(" · ").ifEmpty { null },
        posterUrl = header.posterUrl,
        readiness = if (playerError != null && base.problem == null) {
            base.copy(problem = ReadyProblem("The player stopped", playerError, deadSwarm = false))
        } else {
            base
        },
        gone = probe == ProbePhase.Gone,
        regrabFailed = regrabFailed,
        busy = busy,
        note = t?.takeIf { !header.isMovie && !it.finished && it.files.count { f -> isVideoPath(f.path) } > 1 }
            ?.let { "The rest of the season keeps downloading in the background." },
    )
    GettingReadyContent(
        ui = ui,
        onCancel = onBack,
        onPickAnother = { dead -> vm.replace(dead, onPickAnother) },
        onRetry = { if (playerError != null && setup != null) playback.retry() else vm.retry() },
        onRegrab = vm::regrab,
    )
}

/** The languages it starts in, as the engine will pick them: "English audio, French subtitles". */
private fun startLanguages(setup: WatchSetup, route: PlayRoute): String? {
    val probe = setup.probe
    val tracks = RouteTracks.of(probe, route)
    val audioLang = tracks.audioLanguage(setup.savedAudioIdx)
        ?: setup.prefAudioLang
        ?: probe.audio.firstOrNull { it.default }?.language
        ?: probe.audio.firstOrNull()?.language
    val subLang = when (val saved = setup.savedSubIdx) {
        -1 -> null
        null -> setup.prefSubLang
            ?.takeIf { it != NO_SUBTITLES }
            ?.let { pref -> SubtitlePick.preferredOrdinal(tracks.subtitles, pref)?.let { tracks.subtitles[it].language } }
        else -> tracks.subtitleLanguage(saved)
    }
    val audio = languageName(audioLang)?.let { "$it audio" } ?: return null
    val subs = languageName(subLang)?.let { "$it subtitles" } ?: "no subtitles"
    return "$audio, $subs"
}
