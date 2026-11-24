package dev.nimrag.app.ui.shell.logic

import androidx.compose.ui.graphics.Color
import dev.nimrag.app.ui.theme.NimragColors
import java.io.File
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.Test

/**
 * The native primary colour must stay the web app's dark theme (`web/src/styles/tokens.css`,
 * the `:root[data-theme='dark']` block): the app is always dark, the web's `:root` is light.
 */
class ThemeTokensTest {
    private fun tokens(): String? {
        var dir: File? = File("").absoluteFile
        while (dir != null) {
            val file = File(dir, "web/src/styles/tokens.css")
            if (file.isFile) return file.readText()
            dir = dir.parentFile
        }
        return null
    }

    private fun token(css: String, name: String): Color {
        val hex = Regex("--$name:\\s*#([0-9a-fA-F]{6})\\s*;").find(css)?.groupValues?.get(1)
        assumeTrue(hex != null, "--$name not found in tokens.css")
        return Color(0xFF000000L or hex!!.toLong(16))
    }

    @Test
    fun `native accent colours match the web tokens`() {
        val css = tokens()
        assumeTrue(css != null, "web/ not present")
        val dark = css!!.substringAfter(":root[data-theme='dark']", missingDelimiterValue = "").substringBefore("}")
        assumeTrue(dark.isNotEmpty(), "dark theme block not found in tokens.css")
        assertEquals(token(dark, "accent"), NimragColors.Accent)
        assertEquals(token(dark, "on-accent"), NimragColors.OnAccent)
        assertEquals(token(dark, "link-hover"), NimragColors.AccentHover)
    }
}
