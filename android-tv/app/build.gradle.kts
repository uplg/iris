import org.openapitools.generator.gradle.plugin.tasks.GenerateTask

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.openapi.generator)
}

android {
    namespace = "studio.kahn.iris.tv"
    compileSdk = 37

    defaultConfig {
        applicationId = "studio.kahn.iris.tv"
        minSdk = 23           // Android TV reaches further back than phones
        targetSdk = 37
        versionCode = 16
        versionName = "0.9.1"
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
        // The OpenAPI-generated DTOs use java.time.OffsetDateTime; desugaring
        // backports it (and the rest of java.time) to minSdk 23.
        isCoreLibraryDesugaringEnabled = true
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

// ---------------------------------------------------------------------------
// OpenAPI → Kotlin DTOs. The backend's committed spec (../web/openapi.json,
// utoipa-derived) is the single source of truth for the request/response
// contract; this regenerates the `@Serializable` model layer on every build
// (the TV analogue of web's `openapi-typescript` step), so the client can't
// drift from the server. MODELS ONLY — the Retrofit `IrisApi` interface and
// all OkHttp wiring (auth refresh, cookie jar, caps + client-version headers,
// dual Media3 client, 426 gating) stay hand-written. Generated straight into
// the `…data` package so unchanged schema names need no import churn.
val openApiOut = layout.buildDirectory.dir("generated/openapi")

openApiGenerate {
    generatorName.set("kotlin")
    inputSpec.set(file("$rootDir/../web/openapi.json").path)
    outputDir.set(openApiOut.get().asFile.path)
    packageName.set("studio.kahn.iris.tv.data")
    modelPackage.set("studio.kahn.iris.tv.data")
    library.set("jvm-retrofit2")
    // Models only — no generated API/infra/docs/tests.
    globalProperties.set(
        mapOf("models" to "", "modelDocs" to "false", "modelTests" to "false"),
    )
    configOptions.set(
        mapOf(
            "serializationLibrary" to "kotlinx_serialization",
            // New backend enum variants must not throw on an older client:
            // emit an `unknown_default_open_api` fallback variant + serializer.
            "enumUnknownDefaultCase" to "true",
            // serde `#[serde(tag = …)]` unions (promoted to a `discriminator`
            // in the spec) → generated kotlinx sealed interfaces with a
            // discriminator-aware serializer instead of broken flat classes.
            "generateOneOfAnyOfWrappers" to "true",
        ),
    )
}

// Register the generated Kotlin as a variant source via the AGP 9 Sources API
// (the legacy `sourceSets.java.srcDir` is ignored by AGP 9's built-in Kotlin
// for generated dirs). This also carries the task dependency automatically, so
// `openApiGenerate` runs before compilation. The generator lays files out under
// `<outputDir>/src/main/kotlin/<pkg>`; Kotlin scans the root recursively and
// takes the package from each file's declaration, so wiring `outputDir` works.
androidComponents {
    onVariants { variant ->
        variant.sources.kotlin?.addGeneratedSourceDirectory(
            tasks.named<GenerateTask>("openApiGenerate"),
        ) { it.outputDir }
    }
}

dependencies {
    coreLibraryDesugaring(libs.desugar.jdk.libs)

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

    // Optional hand-built Media3 decoder extensions, NOT on Maven
    // (Google doesn't publish the native ones prebuilt). The fileTree
    // links whatever AARs are present; each is built once per machine /
    // per Media3 bump and the player gracefully falls back to platform
    // decoders when absent (zero cost where hardware already decodes —
    // the extension renderer only kicks in when the platform refuses):
    //   - `scripts/build-ffmpeg-ext.sh` → lib-decoder-ffmpeg-*.aar:
    //     soft audio for DTS / DTS-HD MA / TrueHD / MLP / AC3 / EAC3 …
    //   - `scripts/build-av1-ext.sh` → lib-decoder-av1-*.aar:
    //     soft AV1 video for boxes / Chromecasts without AV1 silicon.
    // Both AARs coexist here. Build instructions live in
    // `android-tv/README.md` §§ "FFmpeg extension" / "AV1 extension".
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
