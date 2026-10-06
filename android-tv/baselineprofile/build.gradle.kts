// Baseline profile producer for :app. It needs a real box (or an API 33+
// emulator) already paired with an Iris server, so the home rows load:
//
//   cd android-tv && ./gradlew :app:generateReleaseBaselineProfile
//
// The plugin writes app/src/release/generated/baselineProfiles/baseline-prof.txt
// (commit it); profileinstaller ships it inside the sideloaded APK.
plugins {
    alias(libs.plugins.android.test)
    alias(libs.plugins.baselineprofile)
}

android {
    namespace = "studio.kahn.iris.tv.baselineprofile"
    compileSdk = 37

    defaultConfig {
        // Profile collection without root needs API 33+; 28 is the
        // macrobenchmark floor for the module itself.
        minSdk = 28
        targetSdk = 37
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    targetProjectPath = ":app"
}

kotlin {
    jvmToolchain(17)
}

baselineProfile {
    useConnectedDevices = true
}

dependencies {
    implementation(libs.androidx.test.ext.junit)
    implementation(libs.uiautomator)
    implementation(libs.benchmark.macro.junit4)
}
