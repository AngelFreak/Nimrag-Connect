package dev.nimrag.app.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

/**
 * Colours for "good" and "caution" states. Material has no success or warning roles, and dynamic
 * colour cannot be relied on to produce green or amber, so these are fixed (with a dark variant).
 * Meaning is never carried by colour alone: every use has text next to it.
 */
@Immutable
data class StatusColors(
    val good: Color,
    val goodContainer: Color,
    val onGoodContainer: Color,
    val warn: Color,
    val warnContainer: Color,
    val onWarnContainer: Color,
)

private val LightStatus = StatusColors(
    good = Color(0xFF2E7D32),
    goodContainer = Color(0xFFC8E6C9),
    onGoodContainer = Color(0xFF0B3D12),
    warn = Color(0xFF8A5A00),
    warnContainer = Color(0xFFFFDDB0),
    onWarnContainer = Color(0xFF2B1700),
)

private val DarkStatus = StatusColors(
    good = Color(0xFF7CD88B),
    goodContainer = Color(0xFF14461E),
    onGoodContainer = Color(0xFFB7F0BF),
    warn = Color(0xFFF2BE6B),
    warnContainer = Color(0xFF4A3000),
    onWarnContainer = Color(0xFFFFDDB0),
)

private val LocalStatusColors = staticCompositionLocalOf { LightStatus }

val MaterialTheme.status: StatusColors
    @Composable @ReadOnlyComposable get() = LocalStatusColors.current

// Fallback palettes for when dynamic colour is switched off (previews, screenshots).
private val LightFallback = lightColorScheme(
    primary = Color(0xFF0F5C73),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFBDE9F8),
    onPrimaryContainer = Color(0xFF001F29),
    secondary = Color(0xFF4B6169),
    secondaryContainer = Color(0xFFCEE6EF),
    tertiary = Color(0xFF5B5B7E),
    background = Color(0xFFF6FAFC),
    surface = Color(0xFFF6FAFC),
)

private val DarkFallback = darkColorScheme(
    primary = Color(0xFF8ED1E8),
    onPrimary = Color(0xFF003545),
    primaryContainer = Color(0xFF004D62),
    onPrimaryContainer = Color(0xFFBDE9F8),
    secondary = Color(0xFFB2CAD3),
    secondaryContainer = Color(0xFF334A52),
    tertiary = Color(0xFFC4C3EB),
    background = Color(0xFF0F1417),
    surface = Color(0xFF0F1417),
)

/** minSdk is 31, so dynamic colour is always available; [dynamicColor] = false is for previews. */
@Composable
fun NimragTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = true,
    content: @Composable () -> Unit,
) {
    val context = LocalContext.current
    val scheme: ColorScheme = when {
        dynamicColor && darkTheme -> dynamicDarkColorScheme(context)
        dynamicColor -> dynamicLightColorScheme(context)
        darkTheme -> DarkFallback
        else -> LightFallback
    }
    CompositionLocalProvider(LocalStatusColors provides if (darkTheme) DarkStatus else LightStatus) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}
