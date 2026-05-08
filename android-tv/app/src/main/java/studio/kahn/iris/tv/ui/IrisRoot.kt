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
import studio.kahn.iris.tv.ui.screens.SearchScreen
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
    const val WATCH = "watch/{infohash}/{fileIdx}"
    fun detail(infohash: String) = "detail/$infohash"
    fun search(q: String? = null, autoPlay: Boolean = false): String {
        val qPart = q?.let { java.net.URLEncoder.encode(it, "UTF-8") } ?: ""
        return "search?q=$qPart&autoPlay=$autoPlay"
    }
    fun watch(infohash: String, fileIdx: Int) = "watch/$infohash/$fileIdx"
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
                    onOpenSearch = {
                        navController.navigate(Routes.search())
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
