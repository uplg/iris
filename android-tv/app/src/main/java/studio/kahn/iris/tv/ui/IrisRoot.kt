package studio.kahn.iris.tv.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import studio.kahn.iris.tv.BuildConfig
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.nav.Routes
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
        !isAuthenticated -> Routes.Pairing
        pendingWatch != null -> Routes.Watch(pendingWatch.first, pendingWatch.second)
        pendingVoiceQuery != null -> Routes.Search(pendingVoiceQuery, autoPlay = true)
        else -> Routes.Home
    }

    // Session dropped underneath us — the refresh token died (expired / revoked)
    // and the Authenticator cleared the stored session. `startDestination` is
    // only honoured on first composition, so when `isAuthenticated` flips to
    // false mid-session we must navigate explicitly; otherwise the TV is
    // stranded on a screen that can only 401 (the "401 + Retry that never
    // reconnects" report). Route back to pairing so the user can re-link.
    LaunchedEffect(isAuthenticated) {
        if (!isAuthenticated) {
            val current = navController.currentDestination
            if (current != null && !current.hasRoute<Routes.Pairing>()) {
                navController.navigate(Routes.Pairing) {
                    popUpTo(0) { inclusive = true }
                }
            }
        }
    }

    val clientOutdated by container.clientOutdated.collectAsState()

    Box(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            // Phone safe zone: keep every screen clear of the system bars
            // AND the soft keyboard (ime — text inputs resize above it; the
            // manifest's adjustResize is ignored under edge-to-edge).
            // Deliberately NOT safeDrawing: its displayCutout inset never
            // zeroes, which kept a notch-sized dead band on the watch
            // screen even in immersive mode. The status bar covers the
            // cutout in portrait anyway, and the browsing gutters clear it
            // in landscape. All-zero on TV. Playback hides the bars (see
            // LockLandscape) → these insets collapse → true full-bleed.
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
                )
            }
            composable<Routes.Home> {
                HomeScreen(
                    container = container,
                    onPickTorrent = { infohash ->
                        navController.navigate(Routes.Detail(infohash))
                    },
                    onPickFile = { infohash, fileIdx ->
                        navController.navigate(Routes.Watch(infohash, fileIdx))
                    },
                    onOpenSettings = {
                        navController.navigate(Routes.Settings)
                    },
                    onOpenSearch = { query ->
                        navController.navigate(Routes.Search(query))
                    },
                    onOpenLibrary = {
                        navController.navigate(Routes.Library)
                    },
                    onPickResult = { providerId, externalId, tmdbId, kind ->
                        navController.navigate(
                            Routes.SearchDetail(providerId, externalId, tmdbId, kind),
                        )
                    },
                    onOpenSeries = { followId ->
                        navController.navigate(Routes.Series(followId))
                    },
                    onOpenCollection = { collectionId ->
                        navController.navigate(Routes.Collection(collectionId))
                    },
                    onOpenDiscover = {
                        navController.navigate(Routes.Discover)
                    },
                    onOpenLiveTv = {
                        navController.navigate(Routes.LiveTv)
                    },
                )
            }
            composable<Routes.LiveTv> {
                LiveTvScreen(
                    container = container,
                    onOpenChannel = { country, channelId ->
                        navController.navigate(Routes.LiveTvWatch(country, channelId))
                    },
                    onBack = { navController.popBackStack() },
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
            composable<Routes.Discover> {
                DiscoverScreen(
                    container = container,
                    onOpenCollection = { collectionId ->
                        navController.navigate(Routes.Collection(collectionId))
                    },
                    onPickResult = { providerId, externalId, tmdbId, kind ->
                        navController.navigate(
                            Routes.SearchDetail(providerId, externalId, tmdbId, kind),
                        )
                    },
                    onOpenSearch = { query ->
                        navController.navigate(Routes.Search(query))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable<Routes.Library> {
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
                            popUpTo<Routes.Home> { inclusive = true }
                        }
                    },
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
            composable<Routes.Search> { backStackEntry ->
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
                )
            }
        }

        // Server-driven version gate: once any request comes back with
        // HTTP 426, the AppContainer flips the `clientOutdated` flow.
        // We cover the UI with a "please update" lock-out everywhere
        // EXCEPT on the Settings screen, where the in-app updater
        // lives — otherwise the user would be stuck with no path to
        // resolve the situation. AppUpdater downloads the APK from a
        // fixed external URL (`synthe.se`), unaffected by the server
        // gate, so the update flow keeps working.
        val currentRoute by navController.currentBackStackEntryAsState()
        if (clientOutdated && currentRoute?.destination?.hasRoute<Routes.Settings>() != true) {
            ClientOutdatedOverlay(
                onOpenSettings = {
                    navController.navigate(Routes.Settings) {
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
            ActionButton("Open Settings", onOpenSettings)
        }
    }
}

