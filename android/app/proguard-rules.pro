-keep class com.velocitydm.android.** { *; }

# S12-e: the shared KMP module (Views + KMP restructure) — kept whole,
# same posture as the app package.
-keep class com.velocitydm.shared.** { *; }

# kotlinx.serialization (R8 runs on every change via assembleRelease in CI)
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.AnnotationsKt

-keepclassmembers class kotlinx.serialization.json.** {
    *** Companion;
}
-keepclasseswithmembers class kotlinx.serialization.json.** {
    kotlinx.serialization.KSerializer serializer(...);
}
-keep,includedescriptorclasses class com.velocitydm.shared.**$$serializer { *; }

-keepclassmembers class com.velocitydm.shared.** {
    *** Companion;
}
-keepclasseswithmembers class com.velocitydm.shared.** {
    kotlinx.serialization.KSerializer serializer(...);
}

# OkHttp (unchanged from the Compose era)
-dontwarn okhttp3.**
-dontwarn okio.**
