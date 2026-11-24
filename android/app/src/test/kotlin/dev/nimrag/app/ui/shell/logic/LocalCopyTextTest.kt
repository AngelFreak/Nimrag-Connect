package dev.nimrag.app.ui.shell.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class LocalCopyTextTest {
    @Test
    fun `no waiting changes give just the phone's copy`() {
        assertEquals(LocalCopyText.Plain, LocalCopyText.of(0))
        assertEquals(LocalCopyText.Plain, LocalCopyText.of(-1))
    }

    @Test
    fun `waiting changes carry their count`() {
        assertEquals(LocalCopyText.Waiting(1), LocalCopyText.of(1))
        assertEquals(LocalCopyText.Waiting(12), LocalCopyText.of(12))
    }
}
