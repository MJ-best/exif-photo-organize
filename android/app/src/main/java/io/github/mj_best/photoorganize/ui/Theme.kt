package io.github.mj_best.photoorganize.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

/** The 크림 / 코코아 looks of the desktop app (sampled from the app icon). */
@Immutable
data class Palette(
    val dark: Boolean,
    val sidebar: Color,
    val content: Color,
    val toolbar: Color,
    val viewer: Color,
    val panel: Color,
    val text: Color,
    val text2: Color,
    val text3: Color,
    val separator: Color,
    val accent: Color,
    val selected: Color,
    val control: Color,
    val star: Color,
    val placeholder: Color,
    val danger: Color,
    val success: Color,
    val warning: Color,
    val hud: Color,
)

val Cream = Palette(
    dark = false,
    sidebar = Color(0xFFF6EBDA),
    content = Color(0xFFFDF6EC),
    toolbar = Color(0xFFFBF1E3),
    viewer = Color(0xFFF2E8D9),
    panel = Color(0xFFFAF0E1),
    text = Color(0xFF3D2B21),
    text2 = Color(0xFF7A6556),
    text3 = Color(0xFFAE9A89),
    separator = Color(0xFFE9D8C2),
    accent = Color(0xFFEE6C3A),
    selected = Color(0xFFF7D8C0),
    control = Color(0xFFF0E2CE),
    star = Color(0xFFF4A92E),
    placeholder = Color(0xFFF1E4D1),
    danger = Color(0xFFE0523C),
    success = Color(0xFF5E9E55),
    warning = Color(0xFFF29A2E),
    hud = Color(0xDE3D2B21),
)

val Cocoa = Palette(
    dark = true,
    sidebar = Color(0xFF332820),
    content = Color(0xFF29201A),
    toolbar = Color(0xFF2F251E),
    viewer = Color(0xFF1C1612),
    panel = Color(0xFF332820),
    text = Color(0xFFFBF1E3),
    text2 = Color(0xFFC8B3A0),
    text3 = Color(0xFF8C7766),
    separator = Color(0xFF483A2F),
    accent = Color(0xFFF47A45),
    selected = Color(0xFF584130),
    control = Color(0xFF45372C),
    star = Color(0xFFFCB840),
    placeholder = Color(0xFF392D24),
    danger = Color(0xFFF06450),
    success = Color(0xFF8BC27A),
    warning = Color(0xFFF7A53A),
    hud = Color(0xDE1C140F),
)

val LocalPalette = staticCompositionLocalOf { Cream }

@Composable
fun PhotoTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val p = if (dark) Cocoa else Cream
    val scheme = if (dark) {
        darkColorScheme(
            primary = p.accent, onPrimary = Color.White,
            secondary = p.star, onSecondary = p.text,
            background = p.content, onBackground = p.text,
            surface = p.panel, onSurface = p.text,
            surfaceVariant = p.control, onSurfaceVariant = p.text2,
            surfaceContainer = p.panel, surfaceContainerHigh = p.sidebar, surfaceContainerHighest = p.control,
            secondaryContainer = p.selected, onSecondaryContainer = p.text,
            outline = p.separator, outlineVariant = p.separator,
            error = p.danger, onError = Color.White,
        )
    } else {
        lightColorScheme(
            primary = p.accent, onPrimary = Color.White,
            secondary = p.star, onSecondary = p.text,
            background = p.content, onBackground = p.text,
            surface = p.panel, onSurface = p.text,
            surfaceVariant = p.control, onSurfaceVariant = p.text2,
            surfaceContainer = p.panel, surfaceContainerHigh = p.sidebar, surfaceContainerHighest = p.control,
            secondaryContainer = p.selected, onSecondaryContainer = p.text,
            outline = p.separator, outlineVariant = p.separator,
            error = p.danger, onError = Color.White,
        )
    }
    androidx.compose.runtime.CompositionLocalProvider(LocalPalette provides p) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}
