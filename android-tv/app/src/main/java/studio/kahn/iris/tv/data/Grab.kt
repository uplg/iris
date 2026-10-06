package studio.kahn.iris.tv.data

import kotlinx.coroutines.flow.first
import retrofit2.HttpException

/** What grabbing a release leads to. */
sealed interface GrabOutcome {
    /** One video (or none listed yet): play it. */
    data class Play(val infohash: String, val fileIdx: Int) : GrabOutcome

    /** Several videos: let the user pick the file. */
    data class Choose(val infohash: String) : GrabOutcome

    /** 409 `duplicate_in_library`: the same movie has a live copy; the
     *  caller confirms with the user and grabs again with `allowDuplicate`. */
    data class Duplicate(val message: String) : GrabOutcome

    data class Failed(val message: String) : GrabOutcome
}

/** Grab a release, then decide what plays: the one place for it. */
suspend fun AppContainer.grabRelease(body: ResolveBody): GrabOutcome {
    val url = sessionStore.serverUrl.first() ?: return GrabOutcome.Failed("Not signed in")
    return try {
        val snapshot = apiFor(url).ingest(body).snapshot
        val videos = snapshot.files.filter { isVideoPath(it.path) }
        if (videos.size <= 1) {
            GrabOutcome.Play(snapshot.infohash, videos.maxByOrNull { it.sizeBytes }?.index ?: 0)
        } else {
            GrabOutcome.Choose(snapshot.infohash)
        }
    } catch (e: HttpException) {
        val env = e.irisError()
        if (env?.error == "duplicate_in_library") {
            GrabOutcome.Duplicate(env.message ?: "This movie is already in the library")
        } else {
            GrabOutcome.Failed(env?.message ?: e.message ?: "Ingest failed")
        }
    } catch (e: Exception) {
        GrabOutcome.Failed(e.message ?: "Ingest failed")
    }
}
