plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.serialization)
}

// S12-e shared Kotlin module (Kotlin Multiplatform):
//
//   commonMain  pure, platform-independent logic — the Md3 token-engine
//               port (goldens asserted against the checked-in Rust-generated
//               md3-tokens-baseline.json), interval-leasing math, the
//               .vdmctl control-file schema + validation, token-bucket wait
//               math, formatting, URL/filename sanitization, task models
//               on String paths.
//   jvmCommon   custom intermediate source set shared by BOTH JVM-family
//               targets (android + jvm): the SegmentEngine orchestration
//               (OkHttp, RandomAccessFile, atomics, coroutines). Every
//               platform-divergent seam lives behind this boundary, so
//               :shared:jvmTest compiles and runs the common tests on the
//               host JVM — and a future non-JVM target adds one actual set.
//   androidMain / jvmMain   target glue (both thin; the JVM code is in
//               jvmCommon).

kotlin {
    androidTarget {
        compilations.all {
            compilerOptions.configure {
                jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
            }
        }
    }
    jvm()

    sourceSets {
        val jvmCommon by creating {
            dependsOn(commonMain.get())
        }
        val androidMain by getting {
            dependsOn(jvmCommon)
        }
        val jvmMain by getting {
            dependsOn(jvmCommon)
        }

        commonMain.dependencies {
            implementation(libs.kotlinx.serialization.json)
            implementation(libs.kotlinx.coroutines.core)
        }
        jvmCommon.dependencies {
            implementation(libs.okhttp)
        }
        commonTest.dependencies {
            implementation(kotlin("test"))
        }
    }
}

android {
    namespace = "com.velocitydm.shared"
    compileSdk = 34
    defaultConfig {
        minSdk = 26
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
