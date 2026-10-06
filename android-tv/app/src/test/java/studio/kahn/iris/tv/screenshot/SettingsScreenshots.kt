package studio.kahn.iris.tv.screenshot

import androidx.compose.runtime.Composable
import java.io.File
import java.time.ZoneId
import java.time.ZonedDateTime
import java.util.UUID
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import studio.kahn.iris.tv.data.AppUpdater
import studio.kahn.iris.tv.data.DeviceView
import studio.kahn.iris.tv.data.GenreOption
import studio.kahn.iris.tv.data.LanguageOption
import studio.kahn.iris.tv.data.PasskeyView
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.data.PreferencesResponse
import studio.kahn.iris.tv.data.UserResponse
import studio.kahn.iris.tv.ui.screens.SettingsContent
import studio.kahn.iris.tv.ui.screens.SetupContent
import studio.kahn.iris.tv.ui.screens.settings.DialogError
import studio.kahn.iris.tv.ui.screens.settings.DialogField
import studio.kahn.iris.tv.ui.screens.settings.Outcome
import studio.kahn.iris.tv.ui.screens.settings.Picks
import studio.kahn.iris.tv.ui.screens.settings.RecoOptions
import studio.kahn.iris.tv.ui.screens.settings.SettingsActions
import studio.kahn.iris.tv.ui.screens.settings.SettingsDialog
import studio.kahn.iris.tv.ui.screens.settings.SettingsSection
import studio.kahn.iris.tv.ui.screens.settings.SettingsUiState
import studio.kahn.iris.tv.ui.screens.settings.SetupUiState
import studio.kahn.iris.tv.ui.screens.settings.TvFacts
import studio.kahn.iris.tv.data.UpdateState
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError

/** Settings, every section, and the sign-in form, from fake states. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = TV_QUALIFIERS)
class SettingsScreenshots {
    @get:Rule
    val shots = IrisScreenshotRule()

    private val now = ZonedDateTime.of(2026, 10, 6, 14, 0, 0, 0, ZoneId.of("Europe/Paris"))
    private fun at(daysAgo: Long) = now.minusDays(daysAgo).toOffsetDateTime()

    private val user = UserResponse("Leonard", "leonard.cherouvrier@kahn.studio", UUID(0, 1), isAdmin = true)
    private val saved = PreferencesResponse(genres = listOf(18L, 878L), includeAnime = true, languages = listOf("fr"), onboardingCompleted = true)
    private val reco = RecoOptions(
        saved,
        languages = listOf(LanguageOption(label = "French", value = "fr"), LanguageOption(label = "English", value = "en"), LanguageOption(label = "Japanese", value = "ja")),
        genres = listOf(
            GenreOption(28, "Action"), GenreOption(16, "Animation"), GenreOption(35, "Comedy"), GenreOption(80, "Crime"),
            GenreOption(99, "Documentary"), GenreOption(18, "Drama"), GenreOption(14, "Fantasy"), GenreOption(27, "Horror"),
            GenreOption(9648, "Mystery"), GenreOption(10749, "Romance"), GenreOption(878, "Science Fiction"), GenreOption(53, "Thriller"),
        ),
    )
    private val devices = listOf(
        DeviceView(at(-80), at(10), UUID(0, 2), "android-tv", "Living room TV"),
        DeviceView(at(-89), at(1), UUID(0, 3), "android-tv", null),
        DeviceView(at(-60), at(30), UUID(0, 4), "web", null),
    )
    private val passkeys = listOf(
        PasskeyView(backedUp = true, createdAt = at(120), id = UUID(0, 5), name = "iPhone", lastUsedAt = at(2)),
        PasskeyView(backedUp = false, createdAt = at(40), id = UUID(0, 6), name = "YubiKey 5C", lastUsedAt = null),
    )
    private val ready = SettingsUiState(
        serverUrl = "https://iris.kahn.studio",
        account = Loadable.Ready(user),
        playback = Loadable.Ready(PlaybackPrefsResponse(audioLanguage = "fre", subtitleLanguage = "off")),
        reco = Loadable.Ready(reco),
        draft = Picks.of(saved),
        devices = Loadable.Ready(devices),
        passkeys = Loadable.Ready(passkeys),
    )
    private val update = UpdateState(installed = "1.5.0", installedCode = 33, latest = AppUpdater.VersionStatus.UpdateAvailable("1.6.0"), checking = false)
    private val facts = TvFacts("1.5.0 (33) · build 20261006-1400", "Google Chromecast HD", "14 (API 34)")

    @Composable
    private fun Settings(state: SettingsUiState = ready, section: SettingsSection, up: UpdateState = update) {
        SettingsContent(state, up, SettingsActions(), initialSection = section, now = now, facts = facts)
    }

    @Test
    fun you() = shots.snapEverySize("settings_you") { Settings(section = SettingsSection.You) }

    @Test
    fun youRenamed() = shots.snap("settings_you_renamed") {
        Settings(ready.copy(outcome = Outcome(SettingsSection.You, "Display name changed to Leonard.")), SettingsSection.You)
    }

    @Test
    fun loading() = shots.snap("settings_loading") { Settings(SettingsUiState(), SettingsSection.You) }

    @Test
    fun failed() = shots.snap("settings_failed") {
        Settings(SettingsUiState(account = Loadable.Failed(UiError(UiError.OFFLINE_MESSAGE, UiError.NETWORK))), SettingsSection.You)
    }

    @Test
    fun playback() = shots.snapEverySize("settings_playback") { Settings(section = SettingsSection.Playback) }

    @Test
    fun audioPanel() = shots.snapEverySize("settings_playback_audio") {
        Settings(ready.copy(dialog = SettingsDialog.Audio), SettingsSection.Playback)
    }

    @Test
    fun recommendations() = shots.snapEverySize("settings_recommendations") {
        Settings(ready.copy(draft = Picks.of(saved).copy(languages = listOf("fr", "en"))), SettingsSection.Recommendations)
    }

    @Test
    fun devices() = shots.snapEverySize("settings_devices") { Settings(section = SettingsSection.Devices) }

    @Test
    fun devicesWaiting() = shots.snap("settings_devices_waiting") {
        Settings(ready.copy(waitingForDevice = true), SettingsSection.Devices)
    }

    @Test
    fun pairDialog() = shots.snapEverySize("settings_devices_pair") {
        Settings(ready.copy(dialog = SettingsDialog.Pair, dialogError = DialogError("This code is not valid or has expired.")), SettingsSection.Devices)
    }

    @Test
    fun revokeDialog() = shots.snap("settings_devices_revoke") {
        Settings(ready.copy(dialog = SettingsDialog.Revoke(devices.first())), SettingsSection.Devices)
    }

    @Test
    fun passkeys() = shots.snapEverySize("settings_passkeys") { Settings(section = SettingsSection.Passkeys) }

    @Test
    fun noPasskeys() = shots.snap("settings_passkeys_none") {
        Settings(ready.copy(passkeys = Loadable.Ready(emptyList())), SettingsSection.Passkeys)
    }

    @Test
    fun password() = shots.snap("settings_password") { Settings(section = SettingsSection.Password) }

    @Test
    fun passwordDialog() = shots.snapEverySize("settings_password_dialog") {
        Settings(
            ready.copy(dialog = SettingsDialog.Password, dialogError = DialogError("Use at least 8 characters.", DialogField.Second)),
            SettingsSection.Password,
        )
    }

    @Test
    fun update() = shots.snapEverySize("settings_update") { Settings(section = SettingsSection.App) }

    @Test
    fun updateDownloading() = shots.snap("settings_update_downloading") {
        Settings(section = SettingsSection.App, up = update.copy(progress = AppUpdater.Progress.Downloading(31_000_000, 52_000_000)))
    }

    @Test
    fun updateReady() = shots.snap("settings_update_ready") {
        Settings(section = SettingsSection.App, up = update.copy(progress = AppUpdater.Progress.Ready(File("iris.apk"))))
    }

    @Test
    fun updateFailed() = shots.snap("settings_update_failed") {
        Settings(
            section = SettingsSection.App,
            up = UpdateState(installed = "1.5.0", installedCode = 33, checking = false, progress = AppUpdater.Progress.Failed("synthe.se answered HTTP 404")),
        )
    }

    @Test
    fun updateCurrent() = shots.snap("settings_update_current") {
        Settings(section = SettingsSection.App, up = update.copy(latest = AppUpdater.VersionStatus.UpToDate("1.5.0")))
    }

    @Test
    fun updateChecking() = shots.snap("settings_update_checking") {
        Settings(section = SettingsSection.App, up = UpdateState(installed = "1.5.0", installedCode = 33, checking = true))
    }

    @Test
    fun updateUnknown() = shots.snap("settings_update_unknown") {
        Settings(section = SettingsSection.App, up = UpdateState(installed = "1.5.0", installedCode = 33))
    }

    @Test
    fun thisTv() = shots.snapEverySize("settings_this_tv") { Settings(section = SettingsSection.ThisTv) }

    @Test
    fun changeServer() = shots.snap("settings_this_tv_server") {
        Settings(ready.copy(dialog = SettingsDialog.ChangeServer), SettingsSection.ThisTv)
    }

    @Test
    fun setup() = shots.snapEverySize("setup") {
        SetupContent(SetupUiState(email = "leonard@kahn.studio"), {}, {}, {}, {}, {})
    }

    @Test
    fun setupFailed() = shots.snap("setup_failed") {
        SetupContent(
            SetupUiState(email = "leonard@kahn.studio", password = "secret", error = "This email and password don't match an Iris account on this server."),
            {}, {}, {}, {}, {},
        )
    }
}
