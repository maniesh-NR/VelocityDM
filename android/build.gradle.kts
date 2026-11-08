plugins {
    id("com.android.application") version "8.7.0" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    // Kotlin 2.0 requires the Compose compiler Gradle plugin when compose is on
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
}
