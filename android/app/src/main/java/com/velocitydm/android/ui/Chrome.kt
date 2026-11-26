package com.velocitydm.android.ui

import android.content.Context
import android.content.res.Configuration
import androidx.appcompat.R as appcompatR
import com.google.android.material.R as materialR
import com.google.android.material.color.MaterialColors
import com.velocitydm.shared.md3.MD3_DEFAULT_SEED
import com.velocitydm.shared.md3.Md3Scheme

/**
 * Runtime color resolution for the app chrome — the Scheme→MDC mapping
 * promised in the S12-e plan (full table in android/docs/ARCHITECTURE.md):
 *
 *  - Android 12+  : MDC theme attrs (wallpaper Material You, applied by
 *    [Md3Theme]). Every element listed below resolves its color attr,
 *    so the whole app follows the wallpaper.
 *  - Android < 12 : the shared Kotlin Md3 port (seed 0x4156A4 — the
 *    actual desktop `DEFAULT_SEED`; note this visibly changes pre-12
 *    colors vs the old hand-copied Google baseline: engine tone tables,
 *    brand-blue hue).
 *
 * Intentional remainder on < 12: MDC-owned containers this app does not
 * paint (dialog accents, ripples, scrollbar) keep MDC's baseline values.
 * The mapping table records this; the promotion gate may bake a
 * generated overlay if the mixture matters on real hardware.
 */
class Chrome(private val ctx: Context) {

    private val dynamic = android.os.Build.VERSION.SDK_INT >= 31

    private val dark: Boolean =
        (ctx.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
            Configuration.UI_MODE_NIGHT_YES

    private val scheme: Md3Scheme by lazy { Md3Scheme.fromSeedHex(MD3_DEFAULT_SEED, dark) }

    private fun attr(attrId: Int, fallback: Int): Int =
        MaterialColors.getColor(ctx, attrId, fallback)

    /** App title / accents / progress / downloading state label. */
    fun primary(): Int = if (dynamic) attr(appcompatR.attr.colorPrimary, scheme.primary) else scheme.primary

    fun onPrimary(): Int = if (dynamic) attr(materialR.attr.colorOnPrimary, scheme.onPrimary) else scheme.onPrimary

    fun primaryContainer(): Int =
        if (dynamic) attr(materialR.attr.colorPrimaryContainer, scheme.primaryContainer) else scheme.primaryContainer

    fun onPrimaryContainer(): Int =
        if (dynamic) attr(materialR.attr.colorOnPrimaryContainer, scheme.onPrimaryContainer) else scheme.onPrimaryContainer

    /** Card / list surfaces. */
    fun surfaceContainer(): Int =
        if (dynamic) attr(materialR.attr.colorSurfaceContainer, scheme.surfaceContainer) else scheme.surfaceContainer

    /** Primary text on surfaces. */
    fun onSurface(): Int =
        if (dynamic) attr(materialR.attr.colorOnSurface, scheme.onSurface) else scheme.onSurface

    /** Secondary text (stats, empty state, URLs). */
    fun onSurfaceVariant(): Int =
        if (dynamic) attr(materialR.attr.colorOnSurfaceVariant, scheme.onSurfaceVariant) else scheme.onSurfaceVariant

    /** Completed-state label. */
    fun tertiary(): Int =
        if (dynamic) attr(materialR.attr.colorTertiary, scheme.tertiary) else scheme.tertiary

    /** Failed-state label and error text. */
    fun error(): Int =
        if (dynamic) attr(appcompatR.attr.colorError, scheme.error) else scheme.error

    /** Error banner surface (S8-W1 remote stale banner). */
    fun errorContainer(): Int =
        if (dynamic) attr(materialR.attr.colorErrorContainer, scheme.errorContainer) else scheme.errorContainer

    /** Text on the error-container surface. */
    fun onErrorContainer(): Int =
        if (dynamic) attr(materialR.attr.colorOnErrorContainer, scheme.onErrorContainer) else scheme.onErrorContainer

    /** Subtle borders/dividers. */
    fun outlineVariant(): Int =
        if (dynamic) attr(materialR.attr.colorOutlineVariant, scheme.outlineVariant) else scheme.outlineVariant
}
