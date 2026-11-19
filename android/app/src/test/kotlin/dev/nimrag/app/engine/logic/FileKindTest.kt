package dev.nimrag.app.engine.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class FileKindTest {
    @Test
    fun `common FIT sub-types get a name`() {
        assertEquals("FIT 128/4 activity", FileKind.label(128, 4))
        assertEquals("FIT 128/32 monitoring", FileKind.label(128, 32))
        assertEquals("FIT 128/49 sleep", FileKind.label(128, 49))
        assertEquals("FIT 128/44 metrics", FileKind.label(128, 44))
        assertEquals("FIT 128/68 HRV status", FileKind.label(128, 68))
        assertEquals("FIT 128/41 changelog", FileKind.label(128, 41))
        assertEquals("FIT 128/58 device", FileKind.label(128, 58))
    }

    @Test
    fun `unknown FIT sub-types keep the numbers only`() {
        assertEquals("FIT 128/99", FileKind.label(128, 99))
        assertEquals("FIT 128/36", FileKind.label(128, 36))
    }

    @Test
    fun `other data types are labelled by their numbers`() {
        assertEquals("type 255/245", FileKind.label(255, 245))
        assertEquals("type 8/255", FileKind.label(8, 255))
        assertEquals("directory 0/0", FileKind.label(0, 0))
    }
}
