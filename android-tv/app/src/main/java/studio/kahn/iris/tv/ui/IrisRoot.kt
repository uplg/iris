package studio.kahn.iris.tv.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import studio.kahn.iris.tv.BuildConfig
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.TopTab
import studio.kahn.iris.tv.ui.nav.ClientOutdatedOverlay
import studio.kahn.iris.tv.ui.nav.LaunchTarget
import studio.kahn.iris.tv.ui.nav.Routes
import studio.kahn.iris.tv.ui.nav.ShellHost
import studio.kahn.iris.tv.ui.nav.ShellViewModel
import studio.kahn.iris.tv.ui.nav.openTab
import studio.kahn.iris.tv.ui.nav.section
import studio.kahn.iris.tv.ui.screens.CollectionScreen
import studio.kahn.iris.tv.ui.screens.DetailScreen
import studio.kahn.iris.tv.ui.screens.DiscoverScreen
import studio.kahn.iris.tv.ui.screens.HistoryScreen
import studio.kahn.iris.tv.ui.screens.HomeScreen
import studio.kahn.iris.tv.ui.screens.LibraryScreen
import studio.kahn.iris.tv.ui.screens.LiveTvScreen
import studio.kahn.iris.tv.ui.screens.LiveTvWatchScreen
import studio.kahn.iris.tv.ui.screens.PairingScreen
import studio.kahn.iris.tv.ui.screens.SearchDetailScreen
import studio.kahn.iris.tv.ui.screens.SearchScreen
import studio.kahn.iris.tv.ui.screens.SeriesScreen
import studio.kahn.iris.tv.ui.screens.SettingsScreen
import studio.kahn.iris.tv.ui.screens.SetupScreen
import studio.kahn.iris.tv.ui.screens.TorrentsScreen
import studio.kahn.iris.tv.ui.screens.WatchScreen
import studio.kahn.iris.tv.ui.screens.settings.SettingsSection
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor

/**
 * The app: one NavHost over [Routes]. The five top-level sections are
 * [section]s (header + Back rules); everything else is full
 * screen. [launch] is a request from outside (Watch Next, voice search,
 * the remote's search key), handled once the TV is paired and then
 * acknowledged with [onLaunchHandled].
 */
@Composable
fun IrisRoot(
    container: AppContainer,
    isAuthenticated: Boolean,
    launch: LaunchTarget? = null,
    onLaunchHandled: () -> Unit = {},
) {
    val navController = rememberNavController()
    // Fixed for the graph's life: a changing start destination rebuilds the graph.
    val start: Any = remember { if (isAuthenticated) Routes.Home else Routes.Pairing }
    val shell = irisViewModel(container) { c, _ -> ShellViewModel(c) }
    val accountName = shell.accountName.collectAsStateWithLifecycle()
    val currentEntry by navController.currentBackStackEntryAsState()

    // `startDestination` is only read once: when the session dies mid-use (the
    // refresh token expired or was revoked) we must go to pairing ourselves,
    // or the TV stays on a screen that can only answer 401.
    LaunchedEffect(isAuthenticated) {
        if (isAuthenticated) {
            shell.refresh()
        } else {
            shell.clear()
            val current = navController.currentDestination
            if (current != null && !current.hasRoute<Routes.Pairing>()) {
                navController.navigate(Routes.Pairing) {
                    popUpTo(navController.graph.id) { inclusive = true }
                }
            }
        }
    }

    val pairing = currentEntry?.destination?.let { it.hasRoute<Routes.Pairing>() || it.hasRoute<Routes.Setup>() }
    LaunchedEffect(launch, isAuthenticated, pairing) {
        val target = launch ?: return@LaunchedEffect
        if (!isAuthenticated) {
            if (!target.keepsUntilPaired) onLaunchHandled()
            return@LaunchedEffect
        }
        if (pairing != false) return@LaunchedEffect
        navController.navigate(target.route()) {
            popUpTo(Routes.Home)
            launchSingleTop = true
        }
        onLaunchHandled()
    }

    val clientOutdated by container.clientOutdated.collectAsStateWithLifecycle()
    val openSettings = {
        navController.navigate(Routes.Settings) { launchSingleTop = true }
    }
    val shellHost = remember(navController) {
        ShellHost(accountName = accountName, onSelect = navController::openTab, onAccount = openSettings)
    }

    Box(
        Modifier
            .fillMaxSize()
            .background(IrisColor.ground)
            // Phone safe zone: clear of the system bars AND the soft keyboard
            // (the manifest's adjustResize is ignored under edge-to-edge). Not
            // safeDrawing: its displayCutout inset never zeroes, which left a
            // notch-sized band on the player even in immersive mode. All-zero
            // on TV; playback hides the bars (LockLandscape), so they collapse.
            .windowInsetsPadding(WindowInsets.systemBars.union(WindowInsets.ime)),
    ) {
        NavHost(navController = navController, startDestination = start) {
            composable<Routes.Pairing> {
                PairingScreen(
                    container = container,
                    onPaired = {
                        navController.navigate(Routes.Home) {
                            popUpTo<Routes.Pairing> { inclusive = true }
                        }
                    },
                    onUsePassword = {
                        navController.navigate(Routes.Setup)
                    },
                )
            }
            composable<Routes.Setup> {
                SetupScreen(
                    container = container,
                    onAuthenticated = {
                        navController.navigate(Routes.Home) {
                            popUpTo<Routes.Pairing> { inclusive = true }
                        }
                    },
                    onUseCode = { navController.popBackStack() },
                )
            }
            section<Routes.Home>(TopTab.Home, shellHost) {
                HomeScreen(
                    container = container,
                    onPlay = { infohash, fileIdx ->
                        navController.navigate(Routes.Watch(infohash, fileIdx))
                    },
                    onOpenCollection = { collectionId ->
                        navController.navigate(Routes.Collection(collectionId))
                    },
                    onOpenSearch = { query ->
                        navController.navigate(Routes.Search(query))
                    },
                    onOpenLibrary = {
                        navController.navigate(Routes.Library)
                    },
                    onOpenDiscover = {
                        navController.navigate(Routes.Discover)
                    },
                    onOpenLiveTv = {
                        navController.navigate(Routes.LiveTv)
                    },
                    onOpenSettings = {
                        navController.navigate(Routes.Settings)
                    },
                )
            }
            section<Routes.LiveTv>(TopTab.LiveTv, shellHost) {
                LiveTvScreen(
                    container = container,
                    onOpenChannel = { country, channelId ->
                        navController.navigate(Routes.LiveTvWatch(country, channelId))
                    },
                )
            }
            composable<Routes.LiveTvWatch> { backStackEntry ->
                val route = backStackEntry.toRoute<Routes.LiveTvWatch>()
                LiveTvWatchScreen(
                    container = container,
                    country = route.country,
                    initialChannelId = route.channelId,
                    onBack = { navController.popBackStack() },
                )
            }
            section<Routes.Discover>(TopTab.Discover, shellHost) {
                DiscoverScreen(
                    container = container,
                    onSelectTab = { },
                    onOpenSearch = { query ->
                        navController.navigate(Routes.Search(query))
                    },
                    onOpenSettings = {
                        navController.navigate(Routes.Settings)
                    },
                )
            }
            section<Routes.Library>(TopTab.Library, shellHost) {
                LibraryScreen(
                    container = container,
                    onOpenCollection = { collectionId ->
                        navController.navigate(Routes.Collection(collectionId))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable<Routes.Collection> { backStackEntry ->
                val route = backStackEntry.toRoute<Routes.Collection>()
                CollectionScreen(
                    container = container,
                    collectionId = route.collectionId,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.Watch(infohash, fileIdx))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable<Routes.Detail> { backStackEntry ->
                val route = backStackEntry.toRoute<Routes.Detail>()
                DetailScreen(
                    container = container,
                    infohash = route.infohash,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.Watch(infohash, fileIdx))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable<Routes.Torrents> {
                TorrentsScreen(
                    container = container,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.Watch(infohash, fileIdx))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable<Routes.Settings> {
                SettingsScreen(
                    container = container,
                    onOpenHistory = {
                        navController.navigate(Routes.History)
                    },
                    onOpenTorrents = {
                        navController.navigate(Routes.Torrents)
                    },
                    onSignOut = {
                        navController.navigate(Routes.Pairing) {
                            popUpTo(navController.graph.id) { inclusive = true }
                        }
                    },
                    onAccountChanged = shell::refresh,
                    initialSection = if (clientOutdated) SettingsSection.App else SettingsSection.You,
                    onBack = { navController.popBackStack() },
                )
            }
            composable<Routes.History> {
                HistoryScreen(
                    container = container,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.Watch(infohash, fileIdx))
                    },
                    onOpenCollection = { collectionId ->
                        navController.navigate(Routes.Collection(collectionId))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            section<Routes.Search>(TopTab.Search, shellHost) { backStackEntry ->
                val route = backStackEntry.toRoute<Routes.Search>()
                SearchScreen(
                    container = container,
                    initialQuery = route.q?.takeIf { it.isNotBlank() },
                    autoPickTop = route.autoPlay,
                    onPickResult = { providerId, externalId, tmdbId, kind ->
                        navController.navigate(
                            Routes.SearchDetail(providerId, externalId, tmdbId, kind),
                        )
                    },
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.Watch(infohash, fileIdx))
                    },
                    onPickTorrent = { infohash ->
                        navController.navigate(Routes.Detail(infohash))
                    },
                    onPickCollection = { collectionId ->
                        navController.navigate(Routes.Collection(collectionId))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable<Routes.SearchDetail> { backStackEntry ->
                val route = backStackEntry.toRoute<Routes.SearchDetail>()
                SearchDetailScreen(
                    container = container,
                    providerId = route.provider,
                    externalId = route.externalId,
                    tmdbId = route.tmdbId,
                    kind = route.kind,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.Watch(infohash, fileIdx)) {
                            // Don't leave the detail screen on the back
                            // stack — user lands at /watch and Back from
                            // there should go to search.
                            popUpTo<Routes.SearchDetail> { inclusive = true }
                        }
                    },
                    onOpenSeries = { followId ->
                        navController.navigate(Routes.Series(followId)) {
                            popUpTo<Routes.SearchDetail> { inclusive = true }
                        }
                    },
                    onPickTorrent = { infohash ->
                        navController.navigate(Routes.Detail(infohash)) {
                            popUpTo<Routes.SearchDetail> { inclusive = true }
                        }
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable<Routes.Series> { backStackEntry ->
                val route = backStackEntry.toRoute<Routes.Series>()
                SeriesScreen(
                    container = container,
                    followId = route.followId,
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.Watch(infohash, fileIdx))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable<Routes.Watch> { backStackEntry ->
                val route = backStackEntry.toRoute<Routes.Watch>()
                WatchScreen(
                    container = container,
                    infohash = route.infohash,
                    fileIdx = route.fileIdx,
                    onBack = { navController.popBackStack() },
                    onNavigateToFile = { nextInfohash, nextFileIdx ->
                        // Replace the current Watch entry instead of
                        // stacking — Back from the next episode should
                        // skip the one we just finished watching.
                        navController.navigate(Routes.Watch(nextInfohash, nextFileIdx)) {
                            popUpTo<Routes.Watch> { inclusive = true }
                        }
                    },
                    onPickAnother = { query ->
                        navController.navigate(Routes.Search(q = query)) {
                            popUpTo<Routes.Watch> { inclusive = true }
                        }
                    },
                )
            }
        }

        // Everything but Settings (where the updater lives) is locked once the
        // server answered 426; the updater downloads from outside the server.
        if (clientOutdated && currentEntry?.destination?.hasRoute<Routes.Settings>() != true) {
            ClientOutdatedOverlay(installedVersion = BuildConfig.VERSION_NAME, onOpenSettings = openSettings)
        }
    }
}
