package studio.kahn.iris.tv.data

import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.test.core.app.ApplicationProvider
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import studio.kahn.iris.tv.ui.theme.scaledDensity

/** The interface size: what each choice scales, and that the choice stays on the device. */
@RunWith(RobolectricTestRunner::class)
class InterfaceSizeTest {
    @Test
    fun eachSizeScalesDpAndSpTogether() {
        assertEquals(listOf(1f, 1.15f, 1.3f), InterfaceSize.entries.map { it.scale })
        assertEquals(listOf("Default", "Larger", "Largest"), InterfaceSize.entries.map { it.label })
        val tv = Density(density = 2f, fontScale = 1.1f)
        val largest = scaledDensity(tv, InterfaceSize.Largest)
        with(largest) {
            assertEquals(2.6f * 10, 10.dp.toPx(), 0.001f)
            assertEquals(1.1f * 2.6f * 10, 10.sp.toPx(), 0.001f)
        }
        assertEquals(1.1f, largest.fontScale)
        assertEquals(tv, scaledDensity(tv, InterfaceSize.Default))
        // A 960x540 dp TV at the largest size lays out for 738x415 dp.
        assertEquals(738f, 960f / InterfaceSize.Largest.scale, 0.5f)
    }

    @Test
    fun anUnknownNameReadsAsDefault() {
        assertEquals(InterfaceSize.Larger, InterfaceSize.named("Larger"))
        assertEquals(InterfaceSize.Default, InterfaceSize.named("Huge"))
        assertEquals(InterfaceSize.Default, InterfaceSize.named(null))
    }

    @Test
    fun theChoiceIsKeptOnTheDevice() = runBlocking {
        val store = PrefsStore(ApplicationProvider.getApplicationContext())
        assertEquals(InterfaceSize.Default, store.interfaceSize.first())
        store.setInterfaceSize(InterfaceSize.Largest)
        assertEquals(InterfaceSize.Largest, store.interfaceSize.first())
        // Another store over the same file (the next app start) reads it back.
        assertEquals(InterfaceSize.Largest, PrefsStore(ApplicationProvider.getApplicationContext()).interfaceSize.first())
        store.setInterfaceSize(InterfaceSize.Default)
    }
}
