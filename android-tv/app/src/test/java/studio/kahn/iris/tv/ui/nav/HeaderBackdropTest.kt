package studio.kahn.iris.tv.ui.nav

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import studio.kahn.iris.tv.ui.screens.home.heroBackdrop
import studio.kahn.iris.tv.ui.screens.live.LogoTone

class HeaderBackdropTest {
    @Test
    fun aBrightStillDarkensOnlyWhatSitsOnIt() {
        val backdrop = heroBackdrop { _, _ -> 0.8 }
        assertNull("the logo sits on the ground", backdrop.inkAt(0.05f, 0.1f))
        assertEquals("the name sits on the still", LogoTone.Dark.plate, backdrop.inkAt(0.9f, 0.95f))
    }

    @Test
    fun aDarkStillKeepsTheUsualWords() {
        assertNull(heroBackdrop { _, _ -> 0.02 }.inkAt(0.9f, 0.95f))
    }

    @Test
    fun aMidStillTakesTheLightWords() {
        assertEquals(LogoTone.Light.plate, heroBackdrop { _, _ -> 0.12 }.inkAt(0.9f, 0.95f))
    }

    @Test
    fun theStartOfTheFadeReadsAsTheGround() {
        assertNull(heroBackdrop { _, _ -> 0.8 }.inkAt(0.44f, 0.46f))
    }

    @Test
    fun anUnreadableStillChangesNothing() {
        assertNull(HeaderBackdrop(emptyList()).inkAt(0f, 1f))
        assertNull(heroBackdrop { _, _ -> null }.inkAt(0.9f, 0.95f))
    }
}
