package studio.kahn.iris.tv.ui.components

import org.junit.Assert.assertEquals
import org.junit.Test

/** The vectors of `packages/iris-api/src/mascot.test.ts`: one name, one mascot, on the web and here. */
class MascotTest {
    @Test
    fun theVectorsSharedWithTheWeb() {
        assertEquals(4095504808L, mascotHash("Vincent"))
        assertEquals(MascotParts(0, 1, 1, 1, 2), mascotOf("Vincent"))
        assertEquals(MascotParts(3, 1, 1, 2, 1), mascotOf("Muriel"))
        assertEquals(MascotParts(1, 3, 3, 1, 2), mascotOf("Uplg"))
        assertEquals(MascotParts(6, 0, 3, 2, 2), mascotOf("Théo"))
        assertEquals(MascotParts(5, 0, 4, 0, 0), mascotOf(""))
    }

    @Test
    fun caseAndSpacesDoNotChangeIt() {
        assertEquals(mascotOf("théo"), mascotOf(" Théo "))
    }
}
