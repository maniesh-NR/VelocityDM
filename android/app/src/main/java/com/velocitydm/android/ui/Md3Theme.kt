package com.velocitydm.android.ui

import android.app.Activity
import android.os.Build
import com.google.android.material.color.DynamicColors

/**
 * OS-specific Material You wiring (S12-e):
 *
 * Android 12+ (API 31): the system's wallpaper-derived dynamic color —
 * applied to MDC theme attrs per activity. This is the platform's own
 * Material You and is deliberately kept as the Android-specific
 * element of the owner's directive ("Material You across all operating
 * systems for consistency, while keeping operating system specific
 * elements").
 *
 * Android < 12: no dynamic-color support exists, so [Chrome] resolves
 * the app chrome from the shared Kotlin Md3 token-engine port
 * (android/shared, seeded with the desktop `DEFAULT_SEED` 0x4156A4 —
 * the same engine and math every other frontend uses).
 */
object Md3Theme {
    fun apply(activity: Activity) {
        if (Build.VERSION.SDK_INT >= 31) {
            DynamicColors.applyToActivityIfAvailable(activity)
        }
    }
}
