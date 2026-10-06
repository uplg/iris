package studio.kahn.iris.tv.ui.screens.search

import androidx.compose.runtime.Immutable
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.GrabConsent
import studio.kahn.iris.tv.data.GrabOutcome
import studio.kahn.iris.tv.data.GrabTarget
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.TitleCard
import studio.kahn.iris.tv.data.grabRelease
import studio.kahn.iris.tv.ui.state.toUiError
import studio.kahn.iris.tv.ui.format.formatSize

/** Where a grab is: what the person must read or agree to before it goes on. */
@Immutable
sealed interface GrabUi {
    /** The release (its [releaseKey], or "release" on its page) the state belongs to. */
    val key: String?

    data object Idle : GrabUi {
        override val key: String? = null
    }

    data class Busy(override val key: String) : GrabUi

    /** Over 50 GB: "Download 62.1 GB?" */
    data class AskHuge(override val key: String, val bytes: Long) : GrabUi {
        val title: String get() = "This release is ${formatSize(bytes)}. Do you really want all of it?"
        val body: String
            get() = "Large packs (a complete series, a box set) fill the shared disk, and everyone's library gets cleaned up " +
                "sooner. If you only want one season or one episode, grab that release instead."
        val confirm: String get() = "Yes, download ${formatSize(bytes)}"
    }

    /** The same movie is already in the library. */
    data class AskDuplicate(override val key: String, val message: String) : GrabUi {
        val title: String get() = "${message.trimEnd('.')}. Download another copy anyway?"
    }

    /** Nothing was downloaded, and why (a RAR set, a full leech slot naming its holder, the server down). */
    data class Refused(override val key: String, val message: String) : GrabUi
}

/** The player to open: the watch screen shows "getting ready" from there. */
@Immutable
data class PlayRequest(val infohash: String, val fileIdx: Int)

const val ARCHIVE_WORDS = "This release is packed in RAR archives, which Iris cannot stream. " +
    "Choose a release with a plain video file (.mkv or .mp4)."

/**
 * One grab at a time, for a screen's ViewModel (web `Grab`): runs
 * [grabRelease], turns its refusals into words and asks, and keeps the
 * consent given so far for the retry. No optimistic state: [play] is set only
 * once the server took the torrent.
 */
class Grabber(private val container: AppContainer, private val scope: CoroutineScope) {
    private val mutable = MutableStateFlow<GrabUi>(GrabUi.Idle)
    val state: StateFlow<GrabUi> = mutable.asStateFlow()
    private val playing = MutableStateFlow<PlayRequest?>(null)
    val play: StateFlow<PlayRequest?> = playing.asStateFlow()

    private var target: GrabTarget? = null
    private var consent = GrabConsent()

    fun run(key: String, target: GrabTarget) {
        if (mutable.value is GrabUi.Busy) return
        this.target = target
        consent = GrabConsent()
        go(key)
    }

    /** Says why [key] is not grabbed, before anything downloads (a dead swarm). */
    fun refuse(key: String, message: String) {
        if (mutable.value is GrabUi.Busy) return
        consent = GrabConsent()
        mutable.value = GrabUi.Refused(key, message)
    }

    /** Plays a release already on disk: nothing to grab. */
    fun playOwned(owned: OwnedFile) {
        playing.value = PlayRequest(owned.infohash, owned.fileIdx)
    }

    /** The person agreed to what [state] asked. */
    fun confirm() {
        val asked = mutable.value
        consent = when (asked) {
            is GrabUi.AskHuge -> consent.copy(huge = true)
            is GrabUi.AskDuplicate -> consent.copy(duplicate = true)
            else -> return
        }
        go(asked.key ?: return)
    }

    /** Leaves the grab where it was: nothing downloaded, the consent forgotten. */
    fun dismiss() {
        if (mutable.value is GrabUi.Busy) return
        mutable.value = GrabUi.Idle
        consent = GrabConsent()
    }

    /** The screen navigated to [play]. */
    fun played() {
        playing.value = null
    }

    private fun go(key: String) {
        val t = target ?: return
        val given = consent
        mutable.value = GrabUi.Busy(key)
        scope.launch {
            mutable.value = try {
                when (val outcome = container.grabRelease(t, given)) {
                    is GrabOutcome.Play -> {
                        playing.value = PlayRequest(outcome.infohash, outcome.fileIdx)
                        GrabUi.Idle
                    }
                    GrabOutcome.Archive -> GrabUi.Refused(key, ARCHIVE_WORDS)
                    is GrabOutcome.Huge -> GrabUi.AskHuge(key, outcome.bytes)
                    is GrabOutcome.Duplicate -> GrabUi.AskDuplicate(key, outcome.message)
                }
            } catch (e: Exception) {
                GrabUi.Refused(key, e.toUiError().message)
            }
        }
    }
}

/** A search result's grab: its TMDB title rides along so the library files it under it. */
fun SearchResult.grabTarget(): GrabTarget = GrabTarget(providerId, externalId, titleMatch?.tmdbId ?: tmdbId)

/**
 * What the last searches read, for the screens they lead to (web
 * `search/cache.ts`): the details endpoint does not carry a release's
 * seeders, poster, TMDB title or library state, and a title's releases
 * screen starts from the card picked. Bounded, process-wide.
 */
object SearchMemory {
    private const val RELEASES = 600
    private const val TITLES = 200

    private val releases = object : LinkedHashMap<String, SearchResult>(64, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, SearchResult>?) = size > RELEASES
    }
    private val titles = object : LinkedHashMap<Long, TitleCard>(32, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<Long, TitleCard>?) = size > TITLES
    }

    @Synchronized
    fun keep(results: List<SearchResult>) {
        results.forEach { releases[releaseKey(it)] = it }
    }

    @Synchronized
    fun release(providerId: String, externalId: String): SearchResult? = releases["$providerId:$externalId"]

    @Synchronized
    fun keepTitles(cards: List<TitleCard>) {
        cards.forEach { titles[it.tmdbId] = it }
    }

    @Synchronized
    fun title(tmdbId: Long): TitleCard? = titles[tmdbId]
}
