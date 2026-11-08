package com.velocitydm.app.ui.theme

// SPDX-License-Identifier: GPL-3.0-or-later

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

// MD3 baseline palette seeded from the VelocityDM brand blue.
val Primary = Color(0xFF4156A4)
val OnPrimary = Color(0xFFFFFFFF)
val PrimaryContainer = Color(0xFFDEE0FF)
val Secondary = Color(0xFF5B5D72)
val Surface = Color(0xFFFBF8FF)
val OnSurface = Color(0xFF1A1B21)
val SurfaceVariant = Color(0xFFE3E1EC)
val Error = Color(0xFFB3261E)

private val LightColors = lightColorScheme(
    primary = Primary,
    onPrimary = OnPrimary,
    primaryContainer = PrimaryContainer,
    secondary = Secondary,
    surface = Surface,
    onSurface = OnSurface,
    surfaceVariant = SurfaceVariant,
    error = Error,
)

private val DarkColors = darkColorScheme(
    primary = Color(0xFFBAC3FF),
    onPrimary = Color(0xFF26348F),
    primaryContainer = Color(0xFF3D47A5),
    secondary = Color(0xFFC4C4DD),
    surface = Color(0xFF121318),
    onSurface = Color(0xFFE3E1E9),
    surfaceVariant = Color(0xFF46464F),
    error = Color(0xFFF2B8B5),
)

private val AppTypography = Typography(
    titleLarge = TextStyle(fontSize = 20.sp, fontWeight = FontWeight.SemiBold),
    titleMedium = TextStyle(fontSize = 16.sp, fontWeight = FontWeight.Medium),
    bodyMedium = TextStyle(fontSize = 14.sp),
    labelSmall = TextStyle(fontSize = 11.sp, fontWeight = FontWeight.Medium),
)

@Composable
fun VelocityDmTheme(
    themeMode: String = com.velocitydm.app.data.AppSettings.THEME_SYSTEM,
    content: @Composable () -> Unit,
) {
    val darkTheme = when (themeMode) {
        com.velocitydm.app.data.AppSettings.THEME_DARK -> true
        com.velocitydm.app.data.AppSettings.THEME_LIGHT -> false
        else -> isSystemInDarkTheme()
    }
    val context = LocalContext.current
    val colorScheme = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
    } else {
        if (darkTheme) DarkColors else LightColors
    }
    MaterialTheme(colorScheme = colorScheme, typography = AppTypography, content = content)
}
