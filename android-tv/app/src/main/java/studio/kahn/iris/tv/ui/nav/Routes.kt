package studio.kahn.iris.tv.ui.nav

import kotlinx.serialization.Serializable

/**
 * Every destination of the app, as type-safe Navigation Compose routes:
 * `navController.navigate(Routes.Watch(infohash, fileIdx))`, read back with
 * `entry.toRoute<Routes.Watch>()`. Navigation encodes the arguments itself
 * (no URL encoding by hand); a nullable or defaulted property is an
 * optional query argument.
 */
object Routes {
    @Serializable data object Pairing
    @Serializable data object Setup
    @Serializable data object Home
    @Serializable data object Library
    @Serializable data class Detail(val infohash: String)
    @Serializable data object Settings
    @Serializable data object Torrents

    /** [autoPlay]: voice search, play the top hit. [notice]: why it was opened (a refused re-grab). */
    @Serializable data class Search(val q: String? = null, val autoPlay: Boolean = false, val notice: String? = null)

    /** [kind] null = unknown, movie by default (gates the Follow button). */
    @Serializable
    data class SearchDetail(
        val provider: String,
        val externalId: String,
        val tmdbId: Long? = null,
        val kind: String? = null,
    )

    /** A title picked in search's Titles view: its releases for [q]. [kind] and [sort] are the search's, by name. */
    @Serializable
    data class SearchTitle(val tmdbId: Long, val q: String, val kind: String? = null, val sort: String? = null)

    @Serializable data class Series(val followId: String)
    @Serializable data class Collection(val collectionId: String)
    @Serializable data class Watch(val infohash: String, val fileIdx: Int)
    @Serializable data object Discover
    @Serializable data object History
    @Serializable data object LiveTv
    @Serializable data class LiveTvWatch(val country: String, val channelId: String)
}
