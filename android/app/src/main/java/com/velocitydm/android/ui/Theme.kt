package com.velocitydm.android.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// Material Design 3 palette (matches the desktop egui GUI)
private val Purple = Color(0xFFD0BCFF)
private val OnPurple = Color(0xFF381E72)
private val Surface = Color(0xFF1C1B1F)
private val Container = Color(0xFF2B2930)
private val TextDim = Color(0xFFCAC4D0)
private val Green = Color(0xFF4EE9B0)
private val Red = Color(0xFFFFB4AB)

@Composable
fun VelocityTheme(content: @Composable () -> Unit) {
    // dark-first like the desktop app
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Purple,
            onPrimary = OnPurple,
            surface = Surface,
            surfaceContainer = Container,
            onSurface = Color(0xFFE6E0E9),
            onSurfaceVariant = TextDim,
            tertiary = Green,
            error = Red,
        ),
        content = content,
    )
}
