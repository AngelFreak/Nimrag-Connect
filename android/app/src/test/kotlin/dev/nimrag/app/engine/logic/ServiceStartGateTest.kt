package dev.nimrag.app.engine.logic

import dev.nimrag.app.engine.StartSource
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class ServiceStartGateTest {
    @Test
    fun `an app update gives one start, not one per path`() {
        val gate = ServiceStartGate()
        // MY_PACKAGE_REPLACED and Application.onCreate within the same second.
        assertEquals(StartDecision.Start, gate.decide(StartRequest.Boot, now = 1_000))
        assertEquals(StartDecision.Skip, gate.decide(StartRequest.ProcessStart, now = 1_050))
        // The activity resumes before the service reached onStartCommand.
        assertEquals(StartDecision.Skip, gate.decide(StartRequest.AppOpened, now = 1_800))
        gate.started()
        assertEquals(StartDecision.Skip, gate.decide(StartRequest.AppOpened, now = 20_000))
        assertEquals(StartDecision.Skip, gate.decide(StartRequest.ProcessStart, now = 20_100))
    }

    @Test
    fun `opening the app does not restart a running service, however often`() {
        val gate = ServiceStartGate()
        gate.started()
        repeat(20) { assertEquals(StartDecision.Skip, gate.decide(StartRequest.AppOpened, now = it * 60_000L)) }
    }

    @Test
    fun `opening the app starts the service when it is not running`() {
        val gate = ServiceStartGate()
        assertEquals(StartDecision.Start, gate.decide(StartRequest.AppOpened, now = 0))
    }

    @Test
    fun `a request that was never answered may start again after the window`() {
        val gate = ServiceStartGate(startingWindowMs = 5_000)
        assertEquals(StartDecision.Start, gate.decide(StartRequest.ProcessStart, now = 0))
        assertEquals(StartDecision.Skip, gate.decide(StartRequest.ProcessStart, now = 4_999))
        assertEquals(StartDecision.Start, gate.decide(StartRequest.ProcessStart, now = 5_000))
    }

    @Test
    fun `a refused start can be retried at once`() {
        val gate = ServiceStartGate()
        assertEquals(StartDecision.Start, gate.decide(StartRequest.Boot, now = 0))
        gate.refused()
        assertEquals(StartDecision.Start, gate.decide(StartRequest.AppOpened, now = 10))
    }

    @Test
    fun `a stopped service can be started again`() {
        val gate = ServiceStartGate()
        gate.started()
        assertTrue(gate.isRunning)
        gate.stopped()
        assertFalse(gate.isRunning)
        assertEquals(StartDecision.Start, gate.decide(StartRequest.Presence, now = 100))
    }

    @Test
    fun `a real permission change goes through even while the service runs`() {
        val gate = ServiceStartGate()
        gate.started()
        assertEquals(StartDecision.Start, gate.decide(StartRequest.PermissionsChanged, now = 0))
    }

    @Test
    fun `presence while the service runs only nudges the link`() {
        val gate = ServiceStartGate()
        gate.started()
        assertEquals(StartDecision.Nudge, gate.decide(StartRequest.Presence, now = 0))
    }

    @Test
    fun `every start source maps to a request`() {
        assertEquals(
            setOf(StartRequest.ProcessStart, StartRequest.Boot, StartRequest.Presence, StartRequest.PermissionsChanged, StartRequest.AppOpened),
            StartSource.entries.map { it.request }.toSet(),
        )
    }
}
