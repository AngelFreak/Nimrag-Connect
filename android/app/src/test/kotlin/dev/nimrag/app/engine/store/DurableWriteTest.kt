package dev.nimrag.app.engine.store

import org.junit.jupiter.api.Assertions.assertArrayEquals
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.io.TempDir
import java.io.File
import java.io.IOException

class DurableWriteTest {
    @TempDir
    lateinit var tmp: File

    @Test
    fun `sha256 of a known input`() {
        assertEquals(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            DurableWrite.sha256Hex("abc".toByteArray()),
        )
    }

    @Test
    fun `the file is written, renamed and the directory synced after the rename`() {
        val dir = File(tmp, "watch")
        val data = byteArrayOf(1, 2, 3, 4)
        var syncedWithFilePresent = false
        val written = DurableWrite.writeAtomic(dir, "x.fit", data) { d ->
            syncedWithFilePresent = File(d, "x.fit").isFile
        }
        assertTrue(written)
        assertTrue(syncedWithFilePresent, "the directory is synced after the rename")
        assertArrayEquals(data, File(dir, "x.fit").readBytes())
        assertEquals(listOf("x.fit"), dir.list()!!.toList(), "no temp file left behind")
    }

    @Test
    fun `the same content is not written twice`() {
        val dir = File(tmp, "watch")
        val data = byteArrayOf(9, 9)
        assertTrue(DurableWrite.writeAtomic(dir, "a.fit", data) {})
        var syncs = 0
        assertFalse(DurableWrite.writeAtomic(dir, "a.fit", data) { syncs++ })
        assertEquals(0, syncs)
    }

    @Test
    fun `a failing directory sync is reported and leaves no temp file`() {
        val dir = File(tmp, "watch")
        assertThrows(IOException::class.java) {
            DurableWrite.writeAtomic(dir, "b.fit", byteArrayOf(5)) { throw IOException("fsync failed") }
        }
        assertFalse(File(dir, ".b.fit.tmp").exists())
    }

    @Test
    fun `an unwritable target is reported`() {
        val notADir = File(tmp, "file").apply { writeText("x") }
        assertThrows(IOException::class.java) {
            DurableWrite.writeAtomic(File(notADir, "sub"), "c.fit", byteArrayOf(1)) {}
        }
    }

    @Test
    fun `an empty file is stored too`() {
        val dir = File(tmp, "watch")
        assertTrue(DurableWrite.writeAtomic(dir, "e.fit", ByteArray(0)) {})
        assertEquals(0L, File(dir, "e.fit").length())
    }
}
