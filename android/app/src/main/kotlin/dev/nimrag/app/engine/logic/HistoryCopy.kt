package dev.nimrag.app.engine.logic

import dev.nimrag.app.engine.auth.logic.MiniJson
import dev.nimrag.app.model.HistoryRetention

/*
 * The server's history on the phone (docs/design/history-on-phone.md): the decisions of the
 * worker's third job, after the file uploads and the offline edits. Pure Kotlin, no Android and no
 * FFI types: the worker and the local server adapter (engine/local) implement the small
 * interfaces here. History files go only into the phone's copy of the server, never into the
 * engine's file store, so they are never uploaded back.
 */

/** One stored file of the server, as `GET /api/device/files` lists it. */
data class HistoryFile(
    val sha256: String,
    val size: Long,
    /** `watch`, `usb`, `import` or `connect`, as the server stores it. */
    val source: String,
    val dataType: Long?,
    val subType: Long?,
    val fileIndex: Long?,
    val fileNumber: Long?,
    val watchTimestamp: Long?,
    /** The file's last data time (unix seconds); null when the server does not know it. */
    val lastTs: Long?,
)

/** The phone's copy of the server (the FFI's `missing_files`, `ingest_history_file`, `prune_history`). Blocking. */
interface HistoryCopy {
    /** Of [sha256s], the ones the copy does not have, in the same order. */
    fun missing(sha256s: List<String>): List<String>

    /** Store and derive [bytes]; throws when they do not hash to [HistoryFile.sha256]. True if the file was new. */
    fun ingest(bytes: ByteArray, file: HistoryFile): Boolean

    /** Remove the copied files with data before [keepSince] (unix seconds; all copies when null). Returns how many. */
    fun prune(keepSince: Long?): Int
}

sealed interface HistoryPage {
    data class Files(val files: List<HistoryFile>, val nextCursor: String?) : HistoryPage

    /** 401 or 403. */
    data class Rejected(val status: Int) : HistoryPage

    data class Failed(val reason: String) : HistoryPage
}

sealed interface HistoryDownload {
    class Bytes(val bytes: ByteArray) : HistoryDownload

    /** 404: the file is not this user's any more (gone meanwhile). */
    data object Gone : HistoryDownload

    data class Rejected(val status: Int) : HistoryDownload

    data class Failed(val reason: String) : HistoryDownload
}

/** The real server's two history routes. Blocking. */
interface HistoryServer {
    fun list(cursor: String?, limit: Int, since: Long?): HistoryPage

    fun download(sha256: String): HistoryDownload
}

object HistoryPolicy {
    /** One pass copies at most this many files... */
    const val PASS_FILES = 25

    /** ...or about this many bytes (the first file of a pass is taken whatever its size). */
    const val PASS_BYTES = 10L * 1024 * 1024

    /** The list is read in pages of this size (a page stays well under the JSON reader's limit). */
    const val PAGE = 200

    /** A finished look is repeated at most this often, whatever asks for it. */
    const val RECHECK_MS = 6L * 3_600_000L

    /** The longest list page read (the server's own limit is 1000 entries, a few hundred KB). */
    const val MAX_LIST_JSON = 2 * 1024 * 1024

    /** A file larger than this is not downloaded into memory (the biggest real ones are a few MB). */
    const val MAX_FILE_BYTES = 64 * 1024 * 1024

    fun listUrl(base: String, cursor: String?, limit: Int, since: Long?): String {
        val params = buildList {
            add("limit=$limit")
            if (since != null) add("since=$since")
            if (!cursor.isNullOrEmpty()) add("cursor=$cursor")
        }
        return "$base/api/device/files?${params.joinToString("&")}"
    }

    fun contentUrl(base: String, sha256: String): String = "$base/api/device/files/$sha256/content"

    /** The page the server answered, or null when the text is not a list of files. */
    fun parseList(body: String): HistoryPage.Files? {
        val root = try {
            MiniJson.parse(body, MAX_LIST_JSON) as? Map<*, *>
        } catch (_: IllegalArgumentException) {
            null
        } ?: return null
        val rows = root["files"] as? List<*> ?: return null
        val files = rows.map { row -> parseFile(row as? Map<*, *> ?: return null) ?: return null }
        val next = when (val c = root["next_cursor"]) {
            null -> null
            is String -> c.takeIf { it.isNotEmpty() }
            else -> return null
        }
        return HistoryPage.Files(files, next)
    }

    private fun parseFile(row: Map<*, *>): HistoryFile? {
        fun number(key: String): Long? = (row[key] as? Double)?.takeIf { it.isFinite() }?.toLong()
        val sha = row["sha256"] as? String ?: return null
        if (sha.length != 64 || !sha.all { it in '0'..'9' || it in 'a'..'f' }) return null
        val source = row["source"] as? String ?: return null
        return HistoryFile(
            sha256 = sha,
            size = number("size") ?: return null,
            source = source,
            dataType = number("data_type"),
            subType = number("sub_type"),
            fileIndex = number("file_index"),
            fileNumber = number("file_number"),
            watchTimestamp = number("watch_timestamp"),
            lastTs = number("last_ts"),
        )
    }

    /** The server's answer to a list request as a page. */
    fun page(reply: ServerReply): HistoryPage = when {
        EditPolicy.tokenRejected(reply.status) -> HistoryPage.Rejected(reply.status)
        reply.status == 404 -> HistoryPage.Failed("The server does not offer its history yet (HTTP 404), update the server")
        reply.status != 200 -> HistoryPage.Failed(EditPolicy.keptReason(reply))
        else -> parseList(reply.body) ?: HistoryPage.Failed("The server's list of files could not be read")
    }

    /** What an answer's status says about a file download (a 200 is handled where the bytes are read). */
    fun downloadFailure(status: Int): HistoryDownload = when {
        EditPolicy.tokenRejected(status) -> HistoryDownload.Rejected(status)
        status == 404 -> HistoryDownload.Gone
        else -> HistoryDownload.Failed(EditPolicy.keptReason(ServerReply(status)))
    }
}

/** What changed in the person's choice since the copy last matched it. */
enum class HistoryChange { None, Widened, Narrowed }

object HistoryChoice {
    /** Compare the choice the copy matches ([applied]) with the choice now ([wanted]). */
    fun change(applied: HistoryRetention, wanted: HistoryRetention): HistoryChange = when {
        wanted.reach > applied.reach -> HistoryChange.Widened
        wanted.reach < applied.reach -> HistoryChange.Narrowed
        else -> HistoryChange.None
    }

    /** The `keep_since` of `prune_history` for narrowing to [wanted]: null (every copy goes) for Off. */
    fun pruneSince(wanted: HistoryRetention, nowMs: Long): Long? = wanted.sinceSeconds(nowMs)
}

/**
 * When to look at the server's list again. A walk that found nothing left is remembered
 * ([checkedAtMs]); after that the list is read again only when something may have changed (new
 * uploads reached the server, the app started) and then at most every [HistoryPolicy.RECHECK_MS].
 * A widened choice and a walk still under way do not wait.
 */
class HistoryRecheck(checkedAtMs: Long = 0L) {
    /** When the list was last walked to its end with nothing left to copy; 0 = never. */
    var checkedAtMs: Long = checkedAtMs
        private set

    /** Something may have changed since the last look. True at app start. */
    private var asked = true
    private var forced = false
    private var working = false

    /** New uploads reached the server (another phone's files are not among them, but the person's own may reorder things). */
    fun onUploadsReached() {
        asked = true
    }

    /** The choice reaches further back: look at once, whatever was checked before. */
    fun onWidened() {
        forced = true
        checkedAtMs = 0L
    }

    /** A pass copied files and some are left: carry on at once. */
    fun onWorkLeft() {
        working = true
    }

    /** The list was walked and nothing is left. */
    fun onChecked(nowMs: Long) {
        checkedAtMs = nowMs
        forced = false
        working = false
        asked = false
    }

    fun due(nowMs: Long): Boolean = waitMs(nowMs) == 0L

    /** 0 when a look is due now; else the milliseconds until one may be, or null when nothing asks for one. */
    fun waitMs(nowMs: Long): Long? {
        if (forced || working || checkedAtMs <= 0L) return 0L
        if (!asked) return null
        if (nowMs < checkedAtMs) return 0L // the clock went back: do not wait for hours that never come
        val since = nowMs - checkedAtMs
        return if (since >= HistoryPolicy.RECHECK_MS) 0L else HistoryPolicy.RECHECK_MS - since
    }
}

/** The lines the worker writes to the log (shown in the Watch page's sync history). */
object HistoryLog {
    fun pass(copied: Int, left: Int) = "Copied ${files(copied)} of the server's history to the phone ($left left)"

    fun done(total: Int) = "The server's history is on the phone (${files(total)} copied)"

    fun pruned(removed: Int, choice: HistoryRetention) =
        "Removed ${files(removed)} of the server's history from the phone (${label(choice)})"

    private fun files(n: Int) = if (n == 1) "1 file" else "$n files"

    fun choice(choice: HistoryRetention) = "Server history on this phone: ${label(choice)}"

    fun failed(reason: String) = "Could not copy the server's history: $reason"

    fun label(choice: HistoryRetention): String = when (choice) {
        HistoryRetention.Off -> "off"
        HistoryRetention.Last3Months -> "last 3 months"
        HistoryRetention.Last12Months -> "last 12 months"
        HistoryRetention.Everything -> "everything"
    }
}

/**
 * Copies the server's files the phone lacks, newest first, a pass at a time. The first pass walks
 * the whole list (with the window) and remembers which files are missing; each pass then takes up
 * to [HistoryPolicy.PASS_FILES] files or [HistoryPolicy.PASS_BYTES] bytes of them. Only a worker
 * thread uses it.
 */
class HistoryCopier(private val copy: HistoryCopy, private val server: HistoryServer) {
    sealed interface Result {
        /** Some files are left: run another pass at once. */
        data class More(val copied: Int, val skipped: Int, val left: Int) : Result

        /** The list is walked and nothing is left. [total] counts the files copied since the walk began. */
        data class Done(val copied: Int, val skipped: Int, val total: Int) : Result

        /** Stop and back off. [copied] files went in before it. */
        data class Failed(val reason: String, val copied: Int) : Result

        /** 401 or 403: stop, like file uploads. */
        data class TokenRejected(val status: Int, val copied: Int) : Result
    }

    private val queue = ArrayDeque<HistoryFile>()
    private var walked = false
    private var copiedSinceWalk = 0

    /** Forget the walk (the choice changed, or the person's data did): the next pass reads the list again. */
    fun reset() {
        queue.clear()
        walked = false
        copiedSinceWalk = 0
    }

    /** How many files the last walk found missing and no pass has copied yet. */
    fun left(): Int = queue.size

    fun pass(retention: HistoryRetention, nowMs: Long): Result {
        if (!retention.copies) return Result.Done(0, 0, 0)
        if (!walked) {
            when (val walk = walk(retention.sinceSeconds(nowMs))) {
                null -> walked = true
                else -> return walk
            }
        } else if (queue.isNotEmpty()) {
            // Files that arrived meanwhile (a sync of the watch can bring the same file).
            val missing = try {
                copy.missing(queue.map { it.sha256 }).toSet()
            } catch (e: Exception) {
                return Result.Failed("The phone's copy could not be asked (${e.javaClass.simpleName})", 0)
            }
            queue.removeAll { it.sha256 !in missing }
        }

        var copied = 0
        var skipped = 0
        var bytes = 0L
        while (queue.isNotEmpty() && copied + skipped < HistoryPolicy.PASS_FILES) {
            val file = queue.first()
            if (copied > 0 && bytes + file.size > HistoryPolicy.PASS_BYTES) break
            when (val got = server.download(file.sha256)) {
                is HistoryDownload.Bytes -> {
                    try {
                        copy.ingest(got.bytes, file)
                    } catch (e: Exception) {
                        // The text may name paths; only the kind of failure goes on.
                        copiedSinceWalk += copied
                        return Result.Failed("The phone could not take a file (${e.javaClass.simpleName})", copied)
                    }
                    queue.removeFirst()
                    copied++
                    bytes += file.size
                }
                HistoryDownload.Gone -> {
                    queue.removeFirst()
                    skipped++
                }
                is HistoryDownload.Rejected -> {
                    copiedSinceWalk += copied
                    return Result.TokenRejected(got.status, copied)
                }
                is HistoryDownload.Failed -> {
                    copiedSinceWalk += copied
                    return Result.Failed(got.reason, copied)
                }
            }
        }
        copiedSinceWalk += copied
        if (queue.isNotEmpty()) return Result.More(copied, skipped, queue.size)
        val total = copiedSinceWalk
        reset()
        return Result.Done(copied, skipped, total)
    }

    /** Read the list to its end and remember what is missing. Null on success, else the failure. */
    private fun walk(since: Long?): Result? {
        queue.clear()
        var cursor: String? = null
        val seen = HashSet<String>()
        while (true) {
            when (val page = server.list(cursor, HistoryPolicy.PAGE, since)) {
                is HistoryPage.Rejected -> return Result.TokenRejected(page.status, 0)
                is HistoryPage.Failed -> return Result.Failed(page.reason, 0)
                is HistoryPage.Files -> {
                    val missing = try {
                        copy.missing(page.files.map { it.sha256 }).toSet()
                    } catch (e: Exception) {
                        return Result.Failed("The phone's copy could not be asked (${e.javaClass.simpleName})", 0)
                    }
                    page.files.filterTo(queue) { it.sha256 in missing }
                    val next = page.nextCursor ?: return null
                    // A server that sends the same cursor again would keep us here for ever.
                    if (!seen.add(next)) return Result.Failed("The server's list of files did not move on", 0)
                    cursor = next
                }
            }
        }
    }
}
