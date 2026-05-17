plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

android {
    namespace = "studio.kahn.iris.tv"
    compileSdk = 36

    defaultConfig {
        applicationId = "studio.kahn.iris.tv"
        minSdk = 23           // Android TV reaches further back than phones
        targetSdk = 36
        versionCode = 6
        versionName = "0.3.0"
    }

    // We're never publishing this on Play Store — the TV is the only target
    // and sideloading from a self-signed APK is fine. So `release` reuses
    // the standard Android debug keystore (~/.android/debug.keystore, which
    // every Android dev machine has). That gives us R8-optimised builds we
    // can sideload without juggling a real keystore.
    signingConfigs {
        create("releaseDebugSigning") {
            storeFile = file(System.getProperty("user.home") + "/.android/debug.keystore")
            storePassword = "android"
            keyAlias = "androiddebugkey"
            keyPassword = "android"
        }
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
            // Allow `http://10.0.2.2` from the emulator and any LAN IP a dev
            // might point at without TLS during early bring-up.
            isDebuggable = true
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            // R8 in full mode: more aggressive shrinking + the optimisation
            // pass that strips Compose's `$$function-N` indirection layer.
            // Combined with the rules in `proguard-rules.pro` this is what
            // gives Compose its release-mode framerate.
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            signingConfig = signingConfigs.getByName("releaseDebugSigning")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
}

// AGP 9.0+ owns the Kotlin compilation; configure the JVM toolchain here.
kotlin {
    jvmToolchain(17)
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.activity.compose)

    // Compose
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.runtime)
    implementation(libs.compose.foundation)
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons.extended)
    debugImplementation(libs.compose.ui.tooling)

    // Compose for TV
    implementation(libs.tv.foundation)
    implementation(libs.tv.material)

    // Navigation
    implementation(libs.navigation.compose)

    // Media3 (HLS + CMAF via DefaultMediaSourceFactory)
    implementation(libs.media3.exoplayer)
    implementation(libs.media3.exoplayer.hls)
    implementation(libs.media3.ui)
    implementation(libs.media3.session)
    implementation(libs.media3.datasource.okhttp)

    // Optional Media3 FFmpeg extension — provides soft decoders for DTS,
    // DTS-HD MA, TrueHD, MLP and a few other formats Android's stock
    // codecs lack. The AAR is NOT on Maven Central: build it once with
    // `scripts/build-ffmpeg-ext.sh` and drop the resulting
    // `media3-decoder-ffmpeg-<version>.aar` into `app/libs/`. The
    // fileTree below is empty until then — the player gracefully falls
    // back to platform decoders (works on Samsung / LG / Shield which
    // have hardware DTS, fails to play DTS-only files on bare emulators
    // and budget Android TV boxes without the AAR). Build instructions
    // live in `android-tv/README.md` § "FFmpeg extension".
    implementation(fileTree(mapOf("dir" to "libs", "include" to listOf("*.aar"))))

    // Networking
    implementation(libs.retrofit)
    implementation(libs.retrofit.kotlinx.serialization)
    implementation(libs.okhttp)
    implementation(libs.okhttp.logging)
    implementation(libs.kotlinx.serialization.json)

    // DataStore (settings: server URL, session cookies)
    implementation(libs.datastore.preferences)

    // Coil 3 (TMDB posters). Coil 3 makes networking opt-in, so we pull in
    // the OkHttp network module to share our auth-aware client.
    implementation(libs.coil.compose)
    implementation(libs.coil.network.okhttp)

    // TV Channels (Library / Continue Watching rows on the Android TV home).
    implementation(libs.tvprovider)
    implementation(libs.work.runtime)
}
