package studio.kahn.iris.tv.ui.nav

import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.navigation.NavBackStackEntry
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** Every route survives a trip through the back stack, awkward characters included. */
@RunWith(RobolectricTestRunner::class)
class RoutesTest {
    @get:Rule
    val compose = createComposeRule()

    private lateinit var nav: NavHostController
    private var shown: Any? = null

    private fun graph() {
        compose.setContent {
            nav = rememberNavController()
            NavHost(nav, startDestination = Routes.Home) {
                composable<Routes.Pairing> { shown = Routes.Pairing }
                composable<Routes.Setup> { shown = Routes.Setup }
                composable<Routes.Home> { shown = Routes.Home }
                composable<Routes.Library> { shown = Routes.Library }
                composable<Routes.Settings> { shown = Routes.Settings }
                composable<Routes.Torrents> { shown = Routes.Torrents }
                composable<Routes.Discover> { shown = Routes.Discover }
                composable<Routes.History> { shown = Routes.History }
                composable<Routes.LiveTv> { shown = Routes.LiveTv }
                composable<Routes.Detail> { shown = it.route<Routes.Detail>() }
                composable<Routes.Search> { shown = it.route<Routes.Search>() }
                composable<Routes.SearchDetail> { shown = it.route<Routes.SearchDetail>() }
                composable<Routes.Series> { shown = it.route<Routes.Series>() }
                composable<Routes.Collection> { shown = it.route<Routes.Collection>() }
                composable<Routes.Watch> { shown = it.route<Routes.Watch>() }
                composable<Routes.LiveTvWatch> { shown = it.route<Routes.LiveTvWatch>() }
            }
        }
        compose.waitForIdle()
    }

    private inline fun <reified T : Any> NavBackStackEntry.route(): T = toRoute<T>()

    private fun roundTrip(route: Any) {
        compose.runOnIdle { nav.navigate(route) }
        compose.waitForIdle()
        assertEquals(route, shown)
    }

    @Test
    fun everyRouteRoundTrips() {
        graph()
        val awkward = "a/b c?d=e&f%g#h+é"
        listOf(
            Routes.Pairing,
            Routes.Setup,
            Routes.Library,
            Routes.Settings,
            Routes.Torrents,
            Routes.Discover,
            Routes.History,
            Routes.LiveTv,
            Routes.Detail("0123456789abcdef0123456789abcdef01234567"),
            Routes.Search(),
            Routes.Search(awkward, autoPlay = true),
            Routes.Search("Past Lives", notice = "This tracker is turned off in Admin. Here are other releases of $awkward."),
            Routes.SearchDetail("c411", awkward),
            Routes.SearchDetail("v3x", "42", tmdbId = 1399L, kind = "tv"),
            Routes.Series("anime:one piece/2"),
            Routes.Collection(awkward),
            Routes.Watch("abcdef", 3),
            Routes.LiveTvWatch("fr", "TF1.fr"),
            Routes.Home,
        ).forEach(::roundTrip)
    }
}
