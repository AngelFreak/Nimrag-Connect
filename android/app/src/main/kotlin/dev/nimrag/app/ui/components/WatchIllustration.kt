package dev.nimrag.app.ui.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * A flat round watch drawn in two colours: the body (straps, case) in a quiet surface tone and
 * the ring, hands and crown in [accent]. Decoration only; the status chip beside it says
 * what is going on, so TalkBack skips it.
 */
@Composable
fun WatchIllustration(
    modifier: Modifier = Modifier,
    size: Dp = 72.dp,
    accent: Color = MaterialTheme.colorScheme.primary,
) {
    val body = MaterialTheme.colorScheme.surfaceContainerHighest
    val face = MaterialTheme.colorScheme.background
    Canvas(modifier.size(size).decorative()) {
        // Drawn on an 80 x 80 grid, scaled to the canvas.
        val u = this.size.minDimension / 80f
        val strap = CornerRadius(4f * u)
        drawRoundRect(body, Offset(27f * u, 4f * u), Size(26f * u, 14f * u), strap)
        drawRoundRect(body, Offset(27f * u, 62f * u), Size(26f * u, 14f * u), strap)
        drawRoundRect(accent, Offset(64f * u, 34f * u), Size(5f * u, 12f * u), CornerRadius(2f * u))
        val centre = Offset(40f * u, 40f * u)
        drawCircle(body, radius = 24f * u, center = centre)
        drawCircle(accent, radius = 24f * u, center = centre, style = Stroke(width = 3f * u))
        drawCircle(face, radius = 17f * u, center = centre)
        val hands = Path().apply {
            moveTo(40f * u, 29f * u)
            lineTo(40f * u, 40f * u)
            lineTo(47f * u, 45f * u)
        }
        drawPath(hands, accent, style = Stroke(width = 3f * u, cap = StrokeCap.Round, join = StrokeJoin.Round))
    }
}
