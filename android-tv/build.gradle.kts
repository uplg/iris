// Top-level project: lists every plugin we use anywhere downstream so the
// version catalog stays the single source of truth.
// AGP 9.0+ ships with built-in Kotlin support, so we don't apply the
// `kotlin-android` plugin explicitly anymore — AGP enables Kotlin
// automatically when it sees Kotlin sources.
plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.kotlin.serialization) apply false
}
