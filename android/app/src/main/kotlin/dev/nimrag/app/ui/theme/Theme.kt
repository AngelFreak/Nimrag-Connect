package dev.nimrag.app.ui.theme

import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.nimrag.app.R

/**
 * The design canvas's tokens (docs/design/artifact/project, the .dc.html files, IMPLEMENTATION_PLAN.md
 * stage 3b). The app is dark only, like the web interface.
 */
object NimragColors {
    val Background = Color(0xFF0F1417)
    val Card = Color(0xFF171E22)
    val Raised = Color(0xFF12181C)
    val Line = Color(0xFF243037)
    val LineStrong = Color(0xFF2A353B)
    val Text = Color(0xFFE8EDEE)
    val TextMuted = Color(0xFF9AA7AC)
    val Link = Color(0xFF7DBBFF)

    /** The web app's primary colour (`--accent`, `--on-accent`, `--link-hover` in web/src/styles/tokens.css); ThemeTokensTest keeps them equal. */
    val Accent = Color(0xFF7DBBFF)
    val OnAccent = Color(0xFF0F1417)
    val AccentHover = Color(0xFFA9D2FF)
    val Energy = Color(0xFF4FD1B5)
    val Heart = Color(0xFFFF6B6B)
    val Warn = Color(0xFFFFB066)
}

/**
 * Colours for "good" and "caution" states. Material has no success or warning roles. Meaning is
 * never carried by colour alone: every use has text next to it.
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

private val Status = StatusColors(
    good = NimragColors.Energy,
    goodContainer = Color(0xFF123A33),
    onGoodContainer = Color(0xFFBFF2E6),
    warn = NimragColors.Warn,
    warnContainer = Color(0xFF3D2A12),
    onWarnContainer = Color(0xFFFFE0BF),
)

private val LocalStatusColors = staticCompositionLocalOf { Status }

val MaterialTheme.status: StatusColors
    @Composable @ReadOnlyComposable get() = LocalStatusColors.current

private val Scheme = darkColorScheme(
    primary = NimragColors.Accent,
    onPrimary = NimragColors.OnAccent,
    primaryContainer = Color(0xFF1B3A57),
    onPrimaryContainer = Color(0xFFD3E8FF),
    secondary = NimragColors.TextMuted,
    onSecondary = NimragColors.Background,
    secondaryContainer = Color(0xFF1F2B31),
    onSecondaryContainer = NimragColors.Text,
    tertiary = NimragColors.Energy,
    onTertiary = NimragColors.Background,
    background = NimragColors.Background,
    onBackground = NimragColors.Text,
    surface = NimragColors.Background,
    onSurface = NimragColors.Text,
    surfaceVariant = NimragColors.Card,
    onSurfaceVariant = NimragColors.TextMuted,
    // No tonal tint: raised surfaces and buttons keep their exact colours, as on the web.
    surfaceTint = Color.Transparent,
    outline = Color(0xFF66767D),
    outlineVariant = NimragColors.Line,
    error = NimragColors.Heart,
    onError = NimragColors.Background,
    errorContainer = Color(0xFF3F1D21),
    onErrorContainer = Color(0xFFFFD9D9),
    surfaceContainerLowest = NimragColors.Raised,
    surfaceContainerLow = NimragColors.Card,
    surfaceContainer = NimragColors.Card,
    surfaceContainerHigh = Color(0xFF1D262B),
    surfaceContainerHighest = NimragColors.Line,
    scrim = Color.Black,
)

private val Inter = FontFamily(
    Font(R.font.inter_400, FontWeight.Normal),
    Font(R.font.inter_500, FontWeight.Medium),
    Font(R.font.inter_600, FontWeight.SemiBold),
)

private val SpaceGrotesk = FontFamily(
    Font(R.font.space_grotesk_500, FontWeight.Medium),
    Font(R.font.space_grotesk_600, FontWeight.SemiBold),
)

/** Inter for text, Space Grotesk for page titles and large numbers, as on the canvas. */
private val NimragTypography: Typography = Typography().let { base ->
    fun inter(weight: FontWeight, size: Int, line: Int, tracking: Float = 0f) =
        TextStyle(fontFamily = Inter, fontWeight = weight, fontSize = size.sp, lineHeight = line.sp, letterSpacing = tracking.sp)

    fun grotesk(size: Int, line: Int) =
        TextStyle(fontFamily = SpaceGrotesk, fontWeight = FontWeight.SemiBold, fontSize = size.sp, lineHeight = line.sp)

    base.copy(
        displayLarge = grotesk(44, 52),
        displayMedium = grotesk(36, 44),
        displaySmall = grotesk(30, 38),
        headlineLarge = grotesk(28, 36),
        headlineMedium = grotesk(24, 32),
        headlineSmall = grotesk(22, 28),
        titleLarge = grotesk(22, 28),
        titleMedium = inter(FontWeight.SemiBold, 15, 22),
        titleSmall = inter(FontWeight.SemiBold, 14, 20),
        bodyLarge = inter(FontWeight.Normal, 16, 24),
        bodyMedium = inter(FontWeight.Normal, 14, 20),
        bodySmall = inter(FontWeight.Normal, 13, 18),
        labelLarge = inter(FontWeight.Medium, 14, 20),
        labelMedium = inter(FontWeight.Medium, 12, 16),
        labelSmall = inter(FontWeight.Medium, 11, 16),
    )
}

/** 16 dp cards, 12 dp buttons and fields, like the canvas. */
private val NimragShapes = Shapes(
    extraSmall = RoundedCornerShape(8.dp),
    small = RoundedCornerShape(12.dp),
    medium = RoundedCornerShape(16.dp),
    large = RoundedCornerShape(16.dp),
    extraLarge = RoundedCornerShape(24.dp),
)

@Composable
fun NimragTheme(content: @Composable () -> Unit) {
    CompositionLocalProvider(LocalStatusColors provides Status) {
        MaterialTheme(colorScheme = Scheme, typography = NimragTypography, shapes = NimragShapes) {
            // Text and icons outside a Surface would otherwise default to black.
            CompositionLocalProvider(LocalContentColor provides Scheme.onSurface, content = content)
        }
    }
}
