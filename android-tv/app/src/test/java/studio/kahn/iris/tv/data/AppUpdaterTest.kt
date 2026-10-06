package studio.kahn.iris.tv.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AppUpdaterTest {
    @Test
    fun versionsCompareAsSemver() {
        assertTrue(AppUpdater.compareSemver("1.5.0", "1.5.1") < 0)
        assertTrue(AppUpdater.compareSemver("1.10.0", "1.9.9") > 0)
        assertEquals(0, AppUpdater.compareSemver("1.5", "1.5.0"))
        assertTrue(AppUpdater.compareSemver("1.5.0-rc1", "1.5.0") < 0)
        assertTrue(AppUpdater.compareSemver("1.5.0", "1.5.0-rc1") > 0)
        assertTrue(AppUpdater.compareSemver("2.0.0", "10.0.0") < 0)
    }

    @Test
    fun theStatusSaysWhetherToUpdate() {
        assertEquals(AppUpdater.VersionStatus.Unknown, AppUpdater.versionStatus("1.5.0", null))
        assertEquals(AppUpdater.VersionStatus.UpdateAvailable("1.6.0"), AppUpdater.versionStatus("1.5.0", "1.6.0"))
        assertEquals(AppUpdater.VersionStatus.UpToDate("1.5.0"), AppUpdater.versionStatus("1.5.0", "1.5.0"))
        assertEquals(AppUpdater.VersionStatus.UpToDate("1.4.2"), AppUpdater.versionStatus("1.5.0", "1.4.2"))
    }
}
