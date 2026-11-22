package dev.nimrag.app.engine.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class RemovedSettingsTest {
    @Test
    fun `only a stored Alongside is announced, anything else is silent`() {
        assertTrue(RemovedSettings.alongsideWasStored("Alongside"))
        assertFalse(RemovedSettings.alongsideWasStored("Off"))
        assertFalse(RemovedSettings.alongsideWasStored(null))
        assertFalse(RemovedSettings.alongsideWasStored(""))
        assertFalse(RemovedSettings.alongsideWasStored("alongside"))
    }

    @Test
    fun `the key and the log line are what the settings and the log use`() {
        assertEquals("garmin_connect_mode", RemovedSettings.GARMIN_CONNECT_MODE_KEY)
        assertFalse(RemovedSettings.ALONGSIDE_REMOVED.contains('\n'))
    }
}
