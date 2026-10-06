package studio.kahn.iris.tv.ui.nav

import android.content.Intent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.ui.components.TopTab

class ShellRulesTest {
    @Test
    fun backGoesToTheHeaderThenHomeThenOut() {
        TopTab.entries.forEach { tab -> assertEquals(ShellBack.ToHeader, shellBack(tab, headerFocused = false)) }
        assertEquals(ShellBack.Leave, shellBack(TopTab.Home, headerFocused = true))
        TopTab.entries.filter { it != TopTab.Home }.forEach { tab ->
            assertEquals(ShellBack.ToHome, shellBack(tab, headerFocused = true))
        }
    }

    @Test
    fun everyTabHasItsRoute() {
        assertEquals(Routes.Home, TopTab.Home.route())
        assertEquals(Routes.Search(), TopTab.Search.route())
        assertEquals(Routes.Discover, TopTab.Discover.route())
        assertEquals(Routes.Library, TopTab.Library.route())
        assertEquals(Routes.LiveTv, TopTab.LiveTv.route())
    }

    @Test
    fun theAccountAlwaysHasWords() {
        assertEquals("Leonard", accountWords("Leonard", "l@x.fr"))
        assertEquals("leonard.c", accountWords("  ", "leonard.c@kahn.studio"))
        assertEquals("Account", accountWords(null, ""))
        assertEquals("Account", accountWords(null, null))
    }

    @Test
    fun watchDeepLinks() {
        assertEquals(
            LaunchTarget.Watch("abc", 2),
            LaunchTarget.of(Intent.ACTION_VIEW, "iris", "watch", listOf("abc", "2"), null),
        )
        assertNull(LaunchTarget.of(Intent.ACTION_VIEW, "iris", "watch", listOf("abc"), null))
        assertNull(LaunchTarget.of(Intent.ACTION_VIEW, "iris", "watch", listOf("abc", "x"), null))
        assertNull(LaunchTarget.of(Intent.ACTION_VIEW, "iris", "watch", listOf("abc", "-1"), null))
        assertNull(LaunchTarget.of(Intent.ACTION_VIEW, "https", "watch", listOf("abc", "2"), null))
        assertEquals(LaunchTarget.Home, LaunchTarget.of(Intent.ACTION_VIEW, "iris", "home", emptyList(), null))
        assertNull(LaunchTarget.of(Intent.ACTION_MAIN, null, null, emptyList(), null))
    }

    @Test
    fun voiceSearchPlaysTheTopHit() {
        val voice = LaunchTarget.of(LaunchTarget.ACTION_MEDIA_PLAY_FROM_SEARCH, null, null, emptyList(), " Dune ")
        assertEquals(LaunchTarget.Search("Dune"), voice)
        assertEquals(Routes.Search("Dune", autoPlay = true), voice?.route())
        assertEquals(LaunchTarget.Search("x"), LaunchTarget.of(Intent.ACTION_SEARCH, null, null, emptyList(), "x"))
        assertNull(LaunchTarget.of(Intent.ACTION_SEARCH, null, null, emptyList(), "  "))
        assertEquals(Routes.Search(null, autoPlay = false), LaunchTarget.Search().route())
    }

    @Test
    fun onlyRealRequestsWaitForPairing() {
        assertTrue(LaunchTarget.Watch("a", 0).keepsUntilPaired)
        assertTrue(LaunchTarget.Search("Dune").keepsUntilPaired)
        assertFalse(LaunchTarget.Search().keepsUntilPaired)
        assertFalse(LaunchTarget.Home.keepsUntilPaired)
    }
}
