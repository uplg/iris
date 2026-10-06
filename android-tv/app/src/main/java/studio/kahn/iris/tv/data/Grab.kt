package studio.kahn.iris.tv.data

import kotlinx.coroutines.flow.first
import retrofit2.HttpException

/** Above this a grab asks twice (web `HUGE_GRAB_BYTES`): complete packs fill the shared disk. */
const val HUGE_GRAB_BYTES = 50L * 1024 * 1024 * 1024

/** A release to grab. [preview] already read (the release page) is not asked again. */
data class GrabTarget(
    val providerId: String,
    val externalId: String,
    val tmdbId: Long? = null,
    val preview: TorrentPreview? = null,
    /** The file chosen; null = the grab's own pick ([autoFile]). */
    val fileIdx: Int? = null,
)

/** What the person agreed to so far. */
data class GrabConsent(val huge: Boolean = false, val duplicate: Boolean = false)

/** What grabbing a release leads to. */
sealed interface GrabOutcome {
    /** Grabbed: the player opens on this file (it shows "getting ready"). */
    data class Play(val infohash: String, val fileIdx: Int) : GrabOutcome

    /** RAR-only or no video: Iris cannot stream it, nothing was downloaded. */
    data object Archive : GrabOutcome

    /** Over [HUGE_GRAB_BYTES]: ask, then grab again with [GrabConsent.huge]. */
    data class Huge(val bytes: Long) : GrabOutcome

    /** 409 `duplicate_in_library`: ask, then grab again with [GrabConsent.duplicate]. */
    data class Duplicate(val message: String) : GrabOutcome
}

/**
 * Grab a release, then say what plays: the one place for it (web
 * `release/grab.svelte.ts`). The guards the server would hit later are said
 * before anything downloads. Other failures throw (a full leech slot is a
 * 409 whose message names the release holding it): map them with `toUiError`.
 */
suspend fun AppContainer.grabRelease(target: GrabTarget, consent: GrabConsent = GrabConsent()): GrabOutcome {
    val url = sessionStore.serverUrl.first() ?: error("This TV is signed out. Pair it again from Settings.")
    val api = apiFor(url)
    val body = ResolveBody(
        externalId = target.externalId,
        providerId = target.providerId,
        tmdbId = target.tmdbId,
    )
    val preview = target.preview ?: try {
        api.previewTorrent(body)
    } catch (e: HttpException) {
        // A magnet source has no `.torrent` to preview: the ingest reads it.
        if (e.code() == 400) null else throw e
    }
    var fileIdx = target.fileIdx
    if (preview != null) {
        fileIdx = fileIdx ?: autoFile(preview.files.map { it.asReleaseFile() })
        if (!preview.streamable || fileIdx == null) return GrabOutcome.Archive
        if (preview.totalSizeBytes > HUGE_GRAB_BYTES && !consent.huge) return GrabOutcome.Huge(preview.totalSizeBytes)
    }
    val snapshot = try {
        api.ingest(body.copy(allowDuplicate = consent.duplicate)).snapshot
    } catch (e: HttpException) {
        val envelope = e.irisError()
        if (envelope?.error == "duplicate_in_library") {
            return GrabOutcome.Duplicate(envelope.message ?: "This movie is already in the library.")
        }
        throw e
    }
    val idx = fileIdx ?: autoFile(snapshot.files.map { it.asReleaseFile() }) ?: 0
    return GrabOutcome.Play(snapshot.infohash, idx)
}
