# Iris TV

Android TV companion app for [Iris](../). Compose-for-TV + Media3.

## Getting set up (first time, ~10 min)

You'll need **Android Studio Hedgehog or newer** (any 2024+ build with the
"Android TV" template support is fine).

### 1. Open the project

```sh
open -a "Android Studio" /Users/leonard/Github/iris/android-tv
```

Studio downloads Gradle, AGP, the Compose plugin, etc. on first open
(several minutes). Wait for the indicator at the bottom-right to go quiet.

### 2. Create an Android TV emulator

The Android TV simulator is just an AVD with a TV form factor + system image.

1. **Tools → Device Manager → Create Virtual Device**
2. **Category: TV** → pick **Android TV (1080p)** or **Google TV (4K)**
3. Click **Next**, pick a system image:
   - **On Apple Silicon (M1/M2/M3 Mac):** ARM64-V8a image. Picking x86_64
     makes the emulator dog-slow under Rosetta. The TV ARM64 images live
     under `Other Images` in the system image picker.
   - **On Intel Mac/Windows/Linux:** x86_64 image is fastest.
   - API level: 34 (Android 14) or 35 (Android 15) is fine.
4. Click **Next** → **Finish**.

### 3. Run the app

- Pick the TV AVD in the device dropdown next to the green play arrow.
- Hit **Run** ▶ (Shift+F10). Studio compiles the APK, boots the emulator,
  installs, launches.
- The first run takes ~30 s for the emulator boot + ~20 s for Gradle. After
  that hot reloads are seconds.

### 4. Use the D-pad

- Mouse click works for development convenience, but the **proper TV
  navigation** is via the D-pad:
  - Arrow keys → directional navigation
  - **Enter** → select
  - **Esc** → back
  - **Home key** → leave app
- Or use the on-screen virtual remote: **Extended controls** (`...` icon)
  → **Directional pad** in the emulator side panel.

### 5. Network: pointing the app at Iris

The emulator runs in its own network namespace. To talk to your local
Iris dev server (`http://localhost:8080` on the host machine), use:

```
http://10.0.2.2:8080
```

That's the magic IP Android emulators alias to "host loopback". For your
production tunnel use `https://iris.kahn.studio` straight up.

## Development workflow

### Build from CLI

```sh
cd android-tv
./gradlew assembleDebug              # builds app/build/outputs/apk/debug/app-debug.apk
./gradlew installDebug               # build + install to running device/emulator
./gradlew :app:installDebug && \
    adb shell am start -n studio.kahn.iris.tv/.MainActivity   # build + install + launch
```

(If `./gradlew` doesn't exist yet, run `gradle wrapper` once from the
`android-tv` directory to generate it.)

### Logs

```sh
adb logcat -s IrisApp Iris OkHttp Compose
```

Or the **Logcat** tab in Studio with filter `studio.kahn.iris.tv`. OkHttp
logging interceptor is wired at `BASIC` level — every API request is
logged so you can verify the cookie session round-trips work.

### Compose previews

`@Preview(uiMode = UI_MODE_TYPE_TELEVISION)` renders TV-sized previews
inside Studio without booting the emulator. Useful for rapid layout
iteration.

## Sideloading on a real Android TV (Chromecast Google TV / Shield TV)

```sh
# Pair once over LAN (TV must have Developer Options enabled and "ADB debugging" on)
adb connect <tv-ip>:5555
adb -s <tv-ip>:5555 install app/build/outputs/apk/debug/app-debug.apk
```

Or use Studio's **Run** with the TV's IP as the device, after pairing.

## Project layout

```
app/src/main/
├── AndroidManifest.xml              ← LEANBACK_LAUNCHER intent filter
├── res/
│   ├── values/strings.xml
│   ├── values/themes.xml            ← system theme (dark, no action bar)
│   ├── xml/network_security_config  ← cleartext only for 10.0.2.2 + LAN
│   └── drawable/banner.xml          ← TV launcher banner (placeholder)
└── java/studio/kahn/iris/tv/
    ├── IrisApp.kt                   ← Application; builds the AppContainer
    ├── MainActivity.kt              ← Compose root
    ├── data/
    │   ├── AppContainer.kt          ← OkHttp + Retrofit + JSON wiring
    │   ├── IrisApi.kt               ← Retrofit interface + DTOs
    │   ├── SessionCookieJar.kt      ← persists Set-Cookie from /auth/login
    │   └── SessionStore.kt          ← DataStore-backed session prefs
    └── ui/
        ├── IrisRoot.kt              ← NavHost (Setup ↔ Home)
        ├── theme/Theme.kt           ← Iris dark palette
        └── screens/
            ├── SetupScreen.kt       ← URL + email + password form
            └── HomeScreen.kt        ← Continue Watching + Library shelves
```

## What's NOT here yet (next iterations)

- **WatchScreen** with ExoPlayer + HLS + audio/sub picker (next session)
- **Search** (D-pad keyboard or Google Assistant integration)
- **Device pairing flow** (replaces the email/password form with a
  short-code/QR scheme — needs backend additions on Iris side)
- **Continue Watching cards with TMDB posters** (Coil is already wired,
  just need the lookup call)

## Troubleshooting

- **"Unresolved reference: tv-material"**: Sync Gradle from the toolbar
  (the elephant icon). Catalog versions resolve from
  `gradle/libs.versions.toml`.
- **App crashes immediately after install on TV**: probably the missing
  `banner.xml` if you removed it. Android TV requires a banner declared
  in the manifest.
- **Login fails with "Network error"**: check `network_security_config.xml`
  — your dev URL must be in the cleartext allowlist. Production
  `https://iris.kahn.studio` works without any allowlisting.
- **Slow emulator on M-series Mac**: you picked the x86_64 image. Recreate
  the AVD with the ARM64-V8a system image.
