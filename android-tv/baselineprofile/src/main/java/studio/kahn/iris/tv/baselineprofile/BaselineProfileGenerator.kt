package studio.kahn.iris.tv.baselineprofile

import androidx.benchmark.macro.junit4.BaselineProfileRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.uiautomator.By
import androidx.test.uiautomator.Until
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Cold start, the home screen, then a walk along the first row: the paths a
 * 1-2 GB box must not JIT on. The box must already be paired (the pairing
 * screen has no rows to scroll).
 */
@RunWith(AndroidJUnit4::class)
class BaselineProfileGenerator {
    @get:Rule
    val rule = BaselineProfileRule()

    @Test
    fun startupHomeAndRow() = rule.collect(packageName = PACKAGE) {
        pressHome()
        startActivityAndWait()
        device.wait(Until.hasObject(By.pkg(PACKAGE).depth(0)), WAIT_MS)
        device.waitForIdle()
        // Down onto the first row, then right along it and back.
        device.pressDPadDown()
        repeat(STEPS) { device.pressDPadRight(); device.waitForIdle() }
        repeat(STEPS) { device.pressDPadLeft() }
        device.pressDPadDown()
        repeat(STEPS) { device.pressDPadRight(); device.waitForIdle() }
    }

    private companion object {
        const val PACKAGE = "studio.kahn.iris.tv"
        const val WAIT_MS = 10_000L
        const val STEPS = 8
    }
}
