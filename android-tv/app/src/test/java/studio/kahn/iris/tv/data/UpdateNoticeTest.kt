package studio.kahn.iris.tv.data

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class UpdateNoticeTest {
    private val available = UpdateState(installed = "1.0.2", latest = AppUpdater.VersionStatus.UpdateAvailable("1.0.4"))

    @Test
    fun aNewerVersionShowsTheNotice() {
        assertEquals(UpdateNotice("1.0.4", "1.0.2", null), updateNotice(available))
    }

    @Test
    fun laterHidesItForThatVersion() {
        assertNull(updateNotice(available.copy(dismissed = "1.0.4")))
    }

    @Test
    fun aVersionNewerThanTheOnePutOffShowsItAgain() {
        assertEquals("1.0.5", updateNotice(available.copy(latest = AppUpdater.VersionStatus.UpdateAvailable("1.0.5"), dismissed = "1.0.4"))?.latest)
    }

    @Test
    fun upToDateShowsNothing() {
        assertNull(updateNotice(UpdateState(installed = "1.0.4", latest = AppUpdater.VersionStatus.UpToDate("1.0.4"))))
    }

    @Test
    fun aFailedOrRunningCheckShowsNothing() {
        assertNull(updateNotice(UpdateState(installed = "1.0.2", latest = AppUpdater.VersionStatus.Unknown)))
        assertNull(updateNotice(UpdateState(installed = "1.0.2", checking = true)))
    }

    @Test
    fun aDownloadIsFollowedEvenWhenTheVersionIsUnknown() {
        val downloading = AppUpdater.Progress.Downloading(1, 2)
        assertEquals(UpdateNotice(null, "1.0.2", downloading), updateNotice(UpdateState(installed = "1.0.2", progress = downloading)))
        val ready = AppUpdater.Progress.Ready(File("iris.apk"))
        assertEquals(ready, updateNotice(available.copy(dismissed = "1.0.4", progress = ready))?.progress)
    }

    @Test
    fun aFailureShowsOnlyWhereTheUpdateIsWanted() {
        val failed = AppUpdater.Progress.Failed("synthe.se answered HTTP 404")
        assertEquals(failed, updateNotice(available.copy(progress = failed))?.progress)
        assertNull(updateNotice(UpdateState(installed = "1.0.2", progress = failed)))
        assertNull(updateNotice(available.copy(dismissed = "1.0.4", progress = failed)))
    }

    @Test
    fun theHeaderKnowsOfAnUpdateWhateverTheNotice() {
        assertEquals("1.0.4", available.copy(dismissed = "1.0.4").available)
        assertNull(UpdateState(latest = AppUpdater.VersionStatus.UpToDate("1.0.4")).available)
    }
}
