package dev.nimrag.app.ui.components

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

/*
 * Line icons for the settings rows: 24 px shapes with a 1.8 stroke, the same family as the tab
 * icons. material-icons-core has none of these, so they are drawn from SVG path data.
 */

private fun svgIcon(name: String, vararg pathData: String): ImageVector =
    ImageVector.Builder(
        name = name,
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = 24f,
        viewportHeight = 24f,
    ).apply {
        pathData.forEach { data ->
            addPath(
                pathData = addPathNodes(data),
                stroke = SolidColor(Color.Black),
                strokeLineWidth = 1.8f,
                strokeLineCap = StrokeCap.Round,
                strokeLineJoin = StrokeJoin.Round,
            )
        }
    }.build()

/** Two arrows chasing each other. */
val SyncIcon: ImageVector by lazy {
    svgIcon("Sync", "M20 11a8 8 0 0 0-14.5-4.5M4 13a8 8 0 0 0 14.5 4.5M5 3v4h4M19 21v-4h-4")
}

/** Four ascending bars. */
val SignalIcon: ImageVector by lazy { svgIcon("Signal", "M5 18v-3M10 18v-6M15 18V9M20 18V5") }

/** Two chain links. */
val LinkIcon: ImageVector by lazy {
    svgIcon("Link", "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1")
}

/** A circled "i". */
val InfoIcon: ImageVector by lazy {
    svgIcon("Info", "M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0", "M12 11v5M12 8h.01")
}

/** A bulleted list. */
val ListIcon: ImageVector by lazy {
    svgIcon("List", "M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01")
}

/** Two stacked server units. */
val ServerIcon: ImageVector by lazy {
    svgIcon(
        "Server",
        "M6 4h12a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
        "M6 13h12a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2z",
        "M8 7.5h.01M8 16.5h.01",
    )
}

/** A shield with a tick: the signed-in account. */
val ShieldIcon: ImageVector by lazy {
    svgIcon("Shield", "M12 3l7 3v6c0 4-3 7.5-7 9-4-1.5-7-5-7-9V6z", "M9 12l2 2 4-4")
}

/** A bell. */
val BellIcon: ImageVector by lazy {
    svgIcon("Bell", "M6 16v-5a6 6 0 0 1 12 0v5l2 2H4l2-2z", "M10 21h4")
}

/** An arrow leaving a door. */
val SignOutIcon: ImageVector by lazy {
    svgIcon("SignOut", "M14 4h4a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-4M10 16l-4-4 4-4M6 12h10")
}

/** Two sliders, for the builder's Advanced page. */
val SlidersIcon: ImageVector by lazy {
    svgIcon("Sliders", "M4 7h9M17 7h3M4 17h3M11 17h9", "M13 7a2 2 0 1 0 4 0a2 2 0 1 0-4 0", "M7 17a2 2 0 1 0 4 0a2 2 0 1 0-4 0")
}

/** A code bracket pair, for the source link. */
val CodeIcon: ImageVector by lazy { svgIcon("Code", "M8 7l-5 5 5 5M16 7l5 5-5 5M14 4l-4 16") }

/** A page with a folded corner and lines, for the licences. */
val DocumentIcon: ImageVector by lazy {
    svgIcon("Document", "M7 3h7l5 5v11a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", "M14 3v5h5M8 13h8M8 17h6")
}

/** A music note. */
val MusicIcon: ImageVector by lazy {
    svgIcon("Music", "M9 18V5l12-2v13", "M3 18a3 3 0 1 0 6 0a3 3 0 1 0-6 0", "M15 16a3 3 0 1 0 6 0a3 3 0 1 0-6 0")
}

/** A phone, seen from the front. */
val PhoneIcon: ImageVector by lazy {
    svgIcon("Phone", "M8 3h8a1.5 1.5 0 0 1 1.5 1.5v15A1.5 1.5 0 0 1 16 21H8a1.5 1.5 0 0 1-1.5-1.5v-15A1.5 1.5 0 0 1 8 3z", "M11 18h2")
}

/** A cloud, for the weather. */
val CloudIcon: ImageVector by lazy {
    svgIcon("Cloud", "M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z")
}
