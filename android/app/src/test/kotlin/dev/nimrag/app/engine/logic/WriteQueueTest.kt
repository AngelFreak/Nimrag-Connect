package dev.nimrag.app.engine.logic

import dev.nimrag.app.engine.logic.WriteQueue.Pump
import dev.nimrag.app.engine.logic.WriteQueue.SendResult
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class WriteQueueTest {
    private val sent = mutableListOf<List<Byte>>()
    private var result = SendResult.Started
    private val queue = WriteQueue { value ->
        if (result == SendResult.Started) sent += value.toList()
        result
    }

    private fun v(vararg b: Int) = ByteArray(b.size) { b[it].toByte() }

    private fun sentValues(vararg values: Byte) = values.map { listOf(it) }

    @Test
    fun `values go out one at a time, the next only after the write completed`() {
        queue.enqueue(listOf(v(1), v(2), v(3)))
        assertEquals(Pump.Sent, queue.pump())
        assertEquals(sentValues(1), sent)
        assertEquals(Pump.Idle, queue.pump(), "a write is in flight")
        assertEquals(Pump.Sent, queue.onWritten(true))
        assertEquals(sentValues(1, 2), sent)
        assertEquals(Pump.Sent, queue.onWritten(true))
        assertEquals(Pump.Idle, queue.onWritten(true))
        assertEquals(sentValues(1, 2, 3), sent)
    }

    @Test
    fun `order is kept across several steps`() {
        queue.enqueue(listOf(v(1), v(2)))
        queue.pump()
        queue.enqueue(listOf(v(3)))
        queue.onWritten(true)
        queue.onWritten(true)
        assertEquals(sentValues(1, 2, 3), sent)
    }

    @Test
    fun `a busy stack keeps the value queued for a retry`() {
        queue.enqueue(listOf(v(1)))
        result = SendResult.Busy
        assertEquals(Pump.Busy, queue.pump())
        assertEquals(Pump.Busy, queue.pump())
        assertEquals(0, sent.size)
        result = SendResult.Started
        assertEquals(Pump.Sent, queue.pump())
        assertEquals(sentValues(1), sent)
    }

    @Test
    fun `a failed send or a failed write completion is reported`() {
        queue.enqueue(listOf(v(1)))
        result = SendResult.Failed
        assertEquals(Pump.Failed, queue.pump())
        result = SendResult.Started
        queue.clear()
        queue.enqueue(listOf(v(9), v(8)))
        queue.pump()
        assertEquals(Pump.Failed, queue.onWritten(false))
    }

    @Test
    fun `clear drops everything, also the in-flight marker`() {
        queue.enqueue(listOf(v(1), v(2)))
        queue.pump()
        assertEquals(2, queue.size)
        queue.clear()
        assertEquals(0, queue.size)
        queue.enqueue(listOf(v(7)))
        assertEquals(Pump.Sent, queue.pump())
    }
}
