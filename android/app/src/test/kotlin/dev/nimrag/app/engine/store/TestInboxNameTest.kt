package dev.nimrag.app.engine.store

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class TestInboxNameTest {
    @Test
    fun `a named file carries its watch directory entry`() {
        assertEquals(TestInboxName.Meta(128, 4, 7, 2, 1_100_000_000), TestInboxName.parse("128-4-7-2-1100000000.fit", 0))
    }

    @Test
    fun `any other name is a monitoring file made now`() {
        val nowMs = 1_800_000_000_000L
        assertEquals(TestInboxName.Meta(128, 32, 0, 0, 1_800_000_000 - 631_065_600), TestInboxName.parse("today.fit", nowMs))
        assertEquals(TestInboxName.Meta(128, 32, 0, 0, 1_800_000_000 - 631_065_600), TestInboxName.parse("1-2-x-4-5.fit", nowMs))
    }
}
