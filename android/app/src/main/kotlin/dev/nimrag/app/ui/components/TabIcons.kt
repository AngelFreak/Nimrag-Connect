package dev.nimrag.app.ui.components

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathBuilder
import androidx.compose.ui.graphics.vector.path
import androidx.compose.ui.unit.dp

/*
 * The four web tabs' icons: the same 24 px line shapes as web/src/components/icons.tsx, so the
 * native bottom bar looks like the web one.
 */

private fun lineIcon(name: String, draw: PathBuilder.() -> Unit): ImageVector =
    ImageVector.Builder(
        name = name,
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = 24f,
        viewportHeight = 24f,
    ).apply {
        path(
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 2f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
            pathBuilder = draw,
        )
    }.build()

/** A sun: a circle with four ticks. */
val TodayIcon: ImageVector by lazy {
    lineIcon("Today") {
        moveTo(8f, 12f)
        arcTo(4f, 4f, 0f, true, true, 16f, 12f)
        arcTo(4f, 4f, 0f, true, true, 8f, 12f)
        close()
        moveTo(12f, 3f); verticalLineTo(5f)
        moveTo(12f, 19f); verticalLineTo(21f)
        moveTo(3f, 12f); horizontalLineTo(5f)
        moveTo(19f, 12f); horizontalLineTo(21f)
    }
}

/** A rising and falling route line. */
val ActivitiesIcon: ImageVector by lazy {
    lineIcon("Activities") {
        moveTo(3f, 17f); lineTo(8f, 11f); lineTo(12f, 14f); lineTo(16f, 7f); lineTo(21f, 13f)
    }
}

/** A heartbeat trace. */
val HealthIcon: ImageVector by lazy {
    lineIcon("Health") {
        moveTo(3f, 12f); horizontalLineTo(7f); lineTo(9f, 7f); lineTo(13f, 17f); lineTo(15f, 12f); horizontalLineTo(21f)
    }
}

/** A barbell. */
val StrengthIcon: ImageVector by lazy {
    lineIcon("Strength") {
        moveTo(6f, 8f); verticalLineTo(16f)
        moveTo(18f, 8f); verticalLineTo(16f)
        moveTo(3f, 10f); verticalLineTo(14f)
        moveTo(21f, 10f); verticalLineTo(14f)
        moveTo(6f, 12f); horizontalLineTo(18f)
    }
}
