package dev.nimrag.app.engine.store

import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.security.MessageDigest

/**
 * Files that survive a crash or power loss once the call returns: temp file in the same
 * directory, data fsynced, renamed to the final name, directory fsynced. Only JDK types, so
 * the write path is unit-tested on the JVM; the directory sync is supplied by the caller.
 */
object DurableWrite {
    fun sha256Hex(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it.toInt() and 0xFF) }

    /**
     * Writes [data] as `dir/name`. Returns false if a file of that name and size already
     * exists (same content hash by construction of the name: nothing is rewritten), true
     * when it was written. Any failure throws and leaves no temp file behind.
     */
    fun writeAtomic(dir: File, name: String, data: ByteArray, syncDir: (File) -> Unit): Boolean {
        if (!dir.isDirectory && !dir.mkdirs() && !dir.isDirectory) throw IOException("cannot create $dir")
        val target = File(dir, name)
        if (target.isFile && target.length() == data.size.toLong()) return false
        val tmp = File(dir, ".$name.tmp")
        try {
            FileOutputStream(tmp).use { out ->
                out.write(data)
                out.flush()
                out.fd.sync()
            }
            if (!tmp.renameTo(target)) throw IOException("rename to $name failed")
            syncDir(dir)
            return true
        } finally {
            if (tmp.exists()) tmp.delete()
        }
    }
}
