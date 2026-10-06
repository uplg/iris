package studio.kahn.iris.tv.data

import android.app.Application
import android.content.Intent
import android.provider.Settings
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf

/** A TV build without the system page the update asks for must not crash the app. */
@RunWith(RobolectricTestRunner::class)
class UpdateIntentsTest {
    private val app: Application = ApplicationProvider.getApplicationContext()

    @Test
    fun noSettingsPageAtAllIsSaidNotThrown() {
        shadowOf(app).checkActivities(true)
        assertFalse(AppUpdater.openInstallPermissionSettings(app))
    }

    @Test
    fun withoutThePerAppPageTheSecurityPageOpens() {
        shadowOf(app).checkActivities(true)
        shadowOf(app.packageManager).addResolveInfoForIntent(
            Intent(Settings.ACTION_SECURITY_SETTINGS),
            android.content.pm.ResolveInfo().apply {
                activityInfo = android.content.pm.ActivityInfo().apply {
                    packageName = "com.android.tv.settings"
                    name = "SecuritySettings"
                }
            },
        )
        assertTrue(AppUpdater.openInstallPermissionSettings(app))
        assertEquals(Settings.ACTION_SECURITY_SETTINGS, shadowOf(app).nextStartedActivity.action)
    }
}
