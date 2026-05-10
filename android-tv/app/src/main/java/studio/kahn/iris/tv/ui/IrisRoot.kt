package studio.kahn.iris.tv.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.screens.DetailScreen
import studio.kahn.iris.tv.ui.screens.HomeScreen
import studio.kahn.iris.tv.ui.screens.PairingScreen
import studio.kahn.iris.tv.ui.screens.SearchDetailScreen
import studio.kahn.iris.tv.ui.screens.SearchScreen
import studio.kahn.iris.tv.ui.screens.SeriesScreen
import studio.kahn.iris.tv.ui.screens.SettingsScreen
import studio.kahn.iris.tv.ui.screens.SetupScreen
import studio.kahn.iris.tv.ui.screens.WatchScreen

object Routes {
    const val PAIRING = "pairing"
    const val SETUP = "setup"
    const val HOME = "home"
    const val DETAIL = "detail/{infohash}"
    const val SETTINGS = "settings"
    const val SEARCH = "search?q={q}&autoPlay={autoPlay}"
    const val SEARCH_DETAIL = "search-detail/{provider}/{externalId}?tmdbId={tmdbId}"
    const val SERIES = "series/{followId}"
    const val WATCH = "watch/{infohash}/{fileIdx}"
    fun detail(infohash: String) = "detail/$infohash"
    fun search(q: String? = null, autoPlay: Boolean = false): String {
        val qPart = q?.let { java.net.URLEncoder.encode(it, "UTF-8") } ?: ""
        return "search?q=$qPart&autoPlay=$autoPlay"
    }
    fun searchDetail(provider: String, externalId: String, tmdbId: Long?): String {
        val p = java.net.URLEncoder.encode(provider, "UTF-8")
        val e = java.net.URLEncoder.encode(externalId, "UTF-8")
        // Pass `0` to mean "no tmdb id" — NavType.LongType doesn't accept null
        // and treating 0 as "absent" is fine since TMDB ids start at 1.
        val t = tmdbId ?: 0L
        return "search-detail/$p/$e?tmdbId=$t"
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
                    onOpenSearch = { query ->
                        navController.navigate(Routes.search(query))
                    },
                    onOpenSeries = { followId ->
                        navController.navigate(Routes.series(followId))
                    },
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
                    onPickResult = { providerId, externalId, tmdbId ->
                        navController.navigate(Routes.searchDetail(providerId, externalId, tmdbId))
                    },
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.watch(infohash, fileIdx))
                    },
                    onPickTorrent = { infohash ->
                        navController.navigate(Routes.detail(infohash))
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
                ),
            ) { backStackEntry ->
                val provider = java.net.URLDecoder.decode(
                    backStackEntry.arguments!!.getString("provider")!!, "UTF-8",
                )
                val externalId = java.net.URLDecoder.decode(
                    backStackEntry.arguments!!.getString("externalId")!!, "UTF-8",
                )
                val tmdbId = backStackEntry.arguments!!.getLong("tmdbId").takeIf { it > 0L }
                SearchDetailScreen(
                    container = container,
                    providerId = provider,
                    externalId = externalId,
                    tmdbId = tmdbId,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.watch(infohash, fileIdx)) {
                            // Don't leave the detail screen on the back
                            // stack — user lands at /watch and Back from
                            // there should go to search.
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
                )
            }
        }
    }
}
