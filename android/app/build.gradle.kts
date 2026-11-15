plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
}

android {
    namespace = "com.velocitydm.android"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.velocitydm.android"
        minSdk = 26
        targetSdk = 34
        // FROZEN with the v1.0.0-family releases (gate 0005): the S12-e
        // restructure is UI-only; the Android promotion gate owns any bump.
        versionCode = 1
        versionName = "1.0.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    packaging {
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
    // One universal APK: the release ships a single `velocitydm-<ver>-android.apk`
    // and per-ABI split `versionCodeOverrides` are not part of the AGP DSL.
}

dependencies {
    // S12-e: Android View-based Material 3 (MDC) — the Compose BOM stack is gone.
    implementation(project(":shared"))
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.appcompat)
    implementation(libs.androidx.activity.ktx)
    implementation(libs.androidx.lifecycle.viewmodel.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.recyclerview)
    implementation(libs.androidx.material)
    implementation(libs.kotlinx.coroutines.android)
}
