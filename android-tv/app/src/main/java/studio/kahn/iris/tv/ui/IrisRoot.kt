package studio.kahn.iris.tv.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import studio.kahn.iris.tv.BuildConfig
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.screens.CollectionScreen
import studio.kahn.iris.tv.ui.screens.DetailScreen
import studio.kahn.iris.tv.ui.screens.ForYouScreen
import studio.kahn.iris.tv.ui.screens.MoodsScreen
import studio.kahn.iris.tv.ui.screens.HomeScreen
import studio.kahn.iris.tv.ui.screens.LibraryScreen
import studio.kahn.iris.tv.ui.screens.PairingScreen
import studio.kahn.iris.tv.ui.screens.SearchDetailScreen
import studio.kahn.iris.tv.ui.screens.SearchScreen
import studio.kahn.iris.tv.ui.screens.SeriesScreen
import studio.kahn.iris.tv.ui.screens.SettingsScreen
import studio.kahn.iris.tv.ui.screens.SetupScreen
import studio.kahn.iris.tv.ui.screens.TorrentsScreen
import studio.kahn.iris.tv.ui.screens.WatchScreen

object Routes {
    const val PAIRING = "pairing"
    const val SETUP = "setup"
    const val HOME = "home"
    const val LIBRARY = "library"
    const val DETAIL = "detail/{infohash}"
    const val SETTINGS = "settings"
    const val TORRENTS = "torrents"
    const val SEARCH = "search?q={q}&autoPlay={autoPlay}"
    const val SEARCH_DETAIL = "search-detail/{provider}/{externalId}?tmdbId={tmdbId}&kind={kind}"
    const val SERIES = "series/{followId}"
    const val COLLECTION = "collection/{collectionId}"
    const val WATCH = "watch/{infohash}/{fileIdx}"
    const val FOR_YOU = "for-you"
    const val MOODS = "moods"
    fun detail(infohash: String) = "detail/$infohash"
    fun collection(id: String): String {
        val cid = java.net.URLEncoder.encode(id, "UTF-8")
        return "collection/$cid"
    }
    fun search(q: String? = null, autoPlay: Boolean = false): String {
        val qPart = q?.let { java.net.URLEncoder.encode(it, "UTF-8") } ?: ""
        return "search?q=$qPart&autoPlay=$autoPlay"
    }
    fun searchDetail(
        provider: String,
        externalId: String,
        tmdbId: Long?,
        kind: String? = null,
    ): String {
        val p = java.net.URLEncoder.encode(provider, "UTF-8")
        val e = java.net.URLEncoder.encode(externalId, "UTF-8")
        // Pass `0` to mean "no tmdb id" — NavType.LongType doesn't accept null
        // and treating 0 as "absent" is fine since TMDB ids start at 1.
        val t = tmdbId ?: 0L
        // Empty `kind` = unknown / movie-by-default. Detail screen
        // uses it to gate the Follow button.
        val k = kind?.let { java.net.URLEncoder.encode(it, "UTF-8") }.orEmpty()
        return "search-detail/$p/$e?tmdbId=$t&kind=$k"
    }
    fun watch(infohash: String, fileIdx: Int) = "watch/$infohash/$fileIdx"
    fun series(followId: String): String {
        val id = java.net.URLEncoder.encode(followId, "UTF-8")
        return "series/$id"
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun IrisRoot(
    container: AppContainer,
    isAuthenticated: Boolean,
    /** When non-null, the activity was launched via voice search (MEDIA_PLAY_FROM_SEARCH). */
    pendingVoiceQuery: String? = null,
    /** When non-null, the activity was launched via a TV channel deep-link. */
    pendingWatch: Pair<String, Int>? = null,
) {
    val navController = rememberNavController()
    val start = when {
        !isAuthenticated -> Routes.PAIRING
        pendingWatch != null -> Routes.watch(pendingWatch.first, pendingWatch.second)
        pendingVoiceQuery != null -> Routes.search(pendingVoiceQuery, autoPlay = true)
        else -> Routes.HOME
    }

    // Session dropped underneath us — the refresh token died (expired / revoked)
    // and the Authenticator cleared the stored session. `startDestination` is
    // only honoured on first composition, so when `isAuthenticated` flips to
    // false mid-session we must navigate explicitly; otherwise the TV is
    // stranded on a screen that can only 401 (the "401 + Retry that never
    // reconnects" report). Route back to pairing so the user can re-link.
    LaunchedEffect(isAuthenticated) {
        if (!isAuthenticated) {
            val current = navController.currentDestination?.route
            if (current != null && current != Routes.PAIRING) {
                navController.navigate(Routes.PAIRING) {
                    popUpTo(0) { inclusive = true }
                }
            }
        }
    }

    val clientOutdated by container.clientOutdated.collectAsState()

    Box(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        NavHost(navController = navController, startDestination = start) {
            composable(Routes.PAIRING) {
                PairingScreen(
                    container = container,
                    onPaired = {
                        navController.navigate(Routes.HOME) {
                            popUpTo(Routes.PAIRING) { inclusive = true }
                        }
                    },
                    onUsePassword = {
                        navController.navigate(Routes.SETUP)
                    },
                )
            }
            composable(Routes.SETUP) {
                SetupScreen(
                    container = container,
                    onAuthenticated = {
                        navController.navigate(Routes.HOME) {
                            popUpTo(Routes.PAIRING) { inclusive = true }
                        }
                    },
                )
            }
            composable(Routes.HOME) {
                HomeScreen(
                    container = container,
                    onPickTorrent = { infohash ->
                        navController.navigate(Routes.detail(infohash))
                    },
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.watch(infohash, fileIdx))
                    },
                    onOpenSettings = {
                        navController.navigate(Routes.SETTINGS)
                    },
                    onOpenTorrents = {
                        navController.navigate(Routes.TORRENTS)
                    },
                    onOpenSearch = { query ->
                        navController.navigate(Routes.search(query))
                    },
                    onOpenLibrary = {
                        navController.navigate(Routes.LIBRARY)
                    },
                    onPickResult = { providerId, externalId, tmdbId, kind ->
                        navController.navigate(
                            Routes.searchDetail(providerId, externalId, tmdbId, kind),
                        )
                    },
                    onOpenSeries = { followId ->
                        navController.navigate(Routes.series(followId))
                    },
                    onOpenCollection = { collectionId ->
                        navController.navigate(Routes.collection(collectionId))
                    },
                    onOpenForYou = {
                        navController.navigate(Routes.FOR_YOU)
                    },
                    onOpenMoods = {
                        navController.navigate(Routes.MOODS)
                    },
                )
            }
            composable(Routes.MOODS) {
                MoodsScreen(
                    container = container,
                    onOpenCollection = { collectionId ->
                        navController.navigate(Routes.collection(collectionId))
                    },
                    onPickResult = { providerId, externalId, tmdbId, kind ->
                        navController.navigate(
                            Routes.searchDetail(providerId, externalId, tmdbId, kind),
                        )
                    },
                    onOpenSearch = { query ->
                        navController.navigate(Routes.search(query))
                    },
                )
            }
            composable(Routes.FOR_YOU) {
                ForYouScreen(
                    container = container,
                    onOpenCollection = { collectionId ->
                        navController.navigate(Routes.collection(collectionId))
                    },
                    onPickResult = { providerId, externalId, tmdbId, kind ->
                        navController.navigate(
                            Routes.searchDetail(providerId, externalId, tmdbId, kind),
                        )
                    },
                    onOpenSearch = { query ->
                        navController.navigate(Routes.search(query))
                    },
                )
            }
            composable(Routes.LIBRARY) {
                LibraryScreen(
                    container = container,
                    onOpenCollection = { collectionId ->
                        navController.navigate(Routes.collection(collectionId))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable(
                Routes.COLLECTION,
                arguments = listOf(navArgument("collectionId") { type = NavType.StringType }),
            ) { backStackEntry ->
                val cid = java.net.URLDecoder.decode(
                    backStackEntry.arguments!!.getString("collectionId")!!, "UTF-8",
                )
                CollectionScreen(
                    container = container,
                    collectionId = cid,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.watch(infohash, fileIdx))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable(
                Routes.DETAIL,
                arguments = listOf(navArgument("infohash") { type = NavType.StringType }),
            ) { backStackEntry ->
                DetailScreen(
                    container = container,
                    infohash = backStackEntry.arguments!!.getString("infohash")!!,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.watch(infohash, fileIdx))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable(Routes.TORRENTS) {
                TorrentsScreen(
                    container = container,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.watch(infohash, fileIdx))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable(Routes.SETTINGS) {
                SettingsScreen(
                    container = container,
                    onSignOut = {
                        navController.navigate(Routes.PAIRING) {
                            popUpTo(Routes.HOME) { inclusive = true }
                        }
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable(
                Routes.SEARCH,
                arguments = listOf(
                    navArgument("q") {
                        type = NavType.StringType
                        defaultValue = ""
                        nullable = false
                    },
                    navArgument("autoPlay") {
                        type = NavType.BoolType
                        defaultValue = false
                    },
                ),
            ) { backStackEntry ->
                val q = backStackEntry.arguments?.getString("q")?.takeIf { it.isNotBlank() }
                    ?.let { java.net.URLDecoder.decode(it, "UTF-8") }
                val autoPlay = backStackEntry.arguments?.getBoolean("autoPlay") ?: false
                SearchScreen(
                    container = container,
                    initialQuery = q,
                    autoPickTop = autoPlay,
                    onPickResult = { providerId, externalId, tmdbId, kind ->
                        navController.navigate(
                            Routes.searchDetail(providerId, externalId, tmdbId, kind),
                        )
                    },
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.watch(infohash, fileIdx))
                    },
                    onPickTorrent = { infohash ->
                        navController.navigate(Routes.detail(infohash))
                    },
                    onPickCollection = { collectionId ->
                        navController.navigate(Routes.collection(collectionId))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable(
                Routes.SEARCH_DETAIL,
                arguments = listOf(
                    navArgument("provider") { type = NavType.StringType },
                    navArgument("externalId") { type = NavType.StringType },
                    navArgument("tmdbId") {
                        type = NavType.LongType
                        defaultValue = 0L
                    },
                    navArgument("kind") {
                        type = NavType.StringType
                        defaultValue = ""
                        nullable = false
                    },
                ),
            ) { backStackEntry ->
                val provider = java.net.URLDecoder.decode(
                    backStackEntry.arguments!!.getString("provider")!!, "UTF-8",
                )
                val externalId = java.net.URLDecoder.decode(
                    backStackEntry.arguments!!.getString("externalId")!!, "UTF-8",
                )
                val tmdbId = backStackEntry.arguments!!.getLong("tmdbId").takeIf { it > 0L }
                val kind = backStackEntry.arguments
                    ?.getString("kind")
                    ?.takeIf { it.isNotBlank() }
                    ?.let { java.net.URLDecoder.decode(it, "UTF-8") }
                SearchDetailScreen(
                    container = container,
                    providerId = provider,
                    externalId = externalId,
                    tmdbId = tmdbId,
                    kind = kind,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.watch(infohash, fileIdx)) {
                            // Don't leave the detail screen on the back
                            // stack — user lands at /watch and Back from
                            // there should go to search.
                            popUpTo(Routes.SEARCH_DETAIL) { inclusive = true }
                        }
                    },
                    onOpenSeries = { followId ->
                        navController.navigate(Routes.series(followId)) {
                            popUpTo(Routes.SEARCH_DETAIL) { inclusive = true }
                        }
                    },
                    onPickTorrent = { infohash ->
                        navController.navigate(Routes.detail(infohash)) {
                            popUpTo(Routes.SEARCH_DETAIL) { inclusive = true }
                        }
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable(
                Routes.SERIES,
                arguments = listOf(navArgument("followId") { type = NavType.StringType }),
            ) { backStackEntry ->
                val followId = java.net.URLDecoder.decode(
                    backStackEntry.arguments!!.getString("followId")!!, "UTF-8",
                )
                SeriesScreen(
                    container = container,
                    followId = followId,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.watch(infohash, fileIdx))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable(
                Routes.WATCH,
                arguments = listOf(
                    navArgument("infohash") { type = NavType.StringType },
                    navArgument("fileIdx") { type = NavType.IntType },
                ),
            ) { backStackEntry ->
                WatchScreen(
                    container = container,
                    infohash = backStackEntry.arguments!!.getString("infohash")!!,
                    fileIdx = backStackEntry.arguments!!.getInt("fileIdx"),
                    onBack = { navController.popBackStack() },
                    onNavigateToFile = { nextInfohash, nextFileIdx ->
                        // Replace the current Watch entry instead of
                        // stacking — Back from the next episode should
                        // skip the one we just finished watching.
                        navController.navigate(Routes.watch(nextInfohash, nextFileIdx)) {
                            popUpTo(Routes.WATCH) { inclusive = true }
                        }
                    },
                )
            }
        }

        // Server-driven version gate: once any request comes back with
        // HTTP 426, the AppContainer flips the `clientOutdated` flow.
        // We cover the UI with a "please update" lock-out everywhere
        // EXCEPT on the Settings screen, where the in-app updater
        // lives — otherwise the user would be stuck with no path to
        // resolve the situation. AppUpdater downloads the APK from a
        // fixed external URL (`uplg.xyz`), unaffected by the server
        // gate, so the update flow keeps working.
        val currentRoute by navController.currentBackStackEntryAsState()
        if (clientOutdated && currentRoute?.destination?.route != Routes.SETTINGS) {
            ClientOutdatedOverlay(
                onOpenSettings = {
                    navController.navigate(Routes.SETTINGS) {
                        // Single Settings entry on the back stack — avoids
                        // a tower of identical screens if the user keeps
                        // hitting the button.
                        launchSingleTop = true
                    }
                },
            )
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ClientOutdatedOverlay(
    onOpenSettings: () -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            // Opaque scrim — the underlying NavHost is still composed (to
            // keep its state warm for after the user updates) but visually
            // hidden, and we capture all focus by being last in the stack.
            .background(Color.Black.copy(alpha = 0.92f)),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            verticalArrangement = Arrangement.spacedBy(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier.padding(48.dp),
        ) {
            Text(
                "Update Iris",
                style = MaterialTheme.typography.displaySmall,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Text(
                "This Iris server requires a newer app. Open Settings to install the latest APK.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "Installed version: ${BuildConfig.VERSION_NAME}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            IrisButton("Open Settings", onOpenSettings)
        }
    }
}

