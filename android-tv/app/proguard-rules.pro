# Keep Kotlinx Serialization companions and @Serializable fields. R8 will
# otherwise strip the generated serializers and the app will crash at runtime
# the first time it tries to decode a response from Iris.
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.AnnotationsKt
-keepclassmembers class **$$serializer { *; }
-keepclasseswithmembers class * {
    kotlinx.serialization.KSerializer serializer(...);
}
-keep,includedescriptorclasses class studio.kahn.iris.tv.**$$serializer { *; }
-keepclassmembers class studio.kahn.iris.tv.** {
    *** Companion;
}
-keepclasseswithmembers class studio.kahn.iris.tv.** {
    kotlinx.serialization.KSerializer serializer(...);
}

# Retrofit reflection: the proxy-based interface bindings need the API
# definitions kept so type information survives.
-keep,allowobfuscation,allowshrinking interface retrofit2.Call
-keep,allowobfuscation,allowshrinking class retrofit2.Response

# Iris swaps the private `trackNameProvider` field on Media3's
# PlayerControlView via reflection so the native track-selection menu
# can render "(Forced)" markers (DefaultTrackNameProvider drops the
# selection flag). R8 must preserve the field and the embedded
# `controller` field on PlayerView for the lookup to keep working.
-keepclassmembers class androidx.media3.ui.PlayerView {
    androidx.media3.ui.PlayerControlView controller;
}
-keepclassmembers class androidx.media3.ui.PlayerControlView {
    androidx.media3.ui.TrackNameProvider trackNameProvider;
}
