package dev.nimrag.app.ui.components

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.path
import androidx.compose.ui.unit.dp

/*
 * material-icons-core has no watch icon, so the few shapes it lacks are drawn here.
 */

/** A wristwatch: two straps and a case ring with an "N" inside, 24 dp. */
val WatchIcon: ImageVector by lazy {
    ImageVector.Builder(
        name = "Watch",
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = 24f,
        viewportHeight = 24f,
    ).apply {
        // Straps.
        path(fill = SolidColor(Color.Black)) {
            moveTo(8.5f, 2f); lineTo(15.5f, 2f); lineTo(16.2f, 6.5f); lineTo(7.8f, 6.5f); close()
            moveTo(7.8f, 17.5f); lineTo(16.2f, 17.5f); lineTo(15.5f, 22f); lineTo(8.5f, 22f); close()
        }
        // Case ring (outer rounded rect minus inner rounded rect).
        path(fill = SolidColor(Color.Black), pathFillType = PathFillType.EvenOdd) {
            moveTo(9f, 5.5f); horizontalLineTo(15f)
            arcTo(3.5f, 3.5f, 0f, false, true, 18.5f, 9f)
            verticalLineTo(15f)
            arcTo(3.5f, 3.5f, 0f, false, true, 15f, 18.5f)
            horizontalLineTo(9f)
            arcTo(3.5f, 3.5f, 0f, false, true, 5.5f, 15f)
            verticalLineTo(9f)
            arcTo(3.5f, 3.5f, 0f, false, true, 9f, 5.5f)
            close()
            moveTo(9f, 7.2f)
            arcTo(1.8f, 1.8f, 0f, false, false, 7.2f, 9f)
            verticalLineTo(15f)
            arcTo(1.8f, 1.8f, 0f, false, false, 9f, 16.8f)
            horizontalLineTo(15f)
            arcTo(1.8f, 1.8f, 0f, false, false, 16.8f, 15f)
            verticalLineTo(9f)
            arcTo(1.8f, 1.8f, 0f, false, false, 15f, 7.2f)
            close()
        }
        // The "N".
        path(
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.5f,
            strokeLineCap = StrokeCap.Butt,
            strokeLineJoin = StrokeJoin.Miter,
        ) {
            moveTo(9.6f, 14.6f); verticalLineTo(9.4f); lineTo(14.4f, 14.6f); verticalLineTo(9.4f)
        }
    }.build()
}
