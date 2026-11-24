package dev.nimrag.app.engine.logic

import dev.nimrag.app.engine.auth.logic.MiniJson

/*
 * Offline edits (docs/design/offline-edits.md): the decisions of the worker's second job, after
 * the file uploads. Pure Kotlin, no Android and no FFI types: the worker and the local server
 * adapter (engine/local) implement the small interfaces here.
 */

/** One change made on the phone's copy that the server has not got yet (the FFI's `PendingEdit`). */
data class OutboxEdit(
    /** Unique per change; the same one is sent again when retrying. */
    val opId: String,
    val method: String,
    /** Already in the server's terms. */
    val path: String,
    /** The request body as JSON text; empty for none. */
    val body: String,
    /** Unix seconds when the change was made. */
    val madeAt: Long,
)

/** What became of a change once the server answered (the FFI's `EditVerdict`). */
enum class EditVerdict {
    /** Done: off the outbox. */
    Applied,

    /** Refused for good: off the outbox, reported by [EditOutbox.takeDropped]. */
    Dropped,

    /** Not decided (no answer, server trouble, token refused): it stays. */
    Kept,
}

/** A change the server could not apply (the FFI's `DroppedEdit`). [path] is the phone's own. */
data class DroppedChange(val method: String, val path: String, val reason: String)

/** The changes made on the phone's copy, as the worker sees them. Every call blocks: not on the main thread. */
interface EditOutbox {
    /** The oldest change, or null. Does not remove it. */
    fun next(): OutboxEdit?

    /** Report the server's answer ([status] 0: there was none). */
    fun done(opId: String, status: Int, responseBody: String): EditVerdict

    fun pending(): Int

    /** The changes the server refused, once. */
    fun takeDropped(): List<DroppedChange>

    /** Replace the copy with the body of `GET /api/device/state`; false (nothing changed) while changes wait. */
    fun applyState(json: String): Boolean
}

/** What the server answered to one request. [status] 0 means no answer; [problem] then says why. */
data class ServerReply(val status: Int, val body: String = "", val problem: String? = null) {
    companion object {
        fun noAnswer(problem: String) = ServerReply(0, "", problem)
    }
}

object EditPolicy {
    /** Pull again on resume only when the last pull is older than this. */
    const val RESUME_PULL_GAP_MS = 30_000L

    /** The body of `POST /api/device/edits`. [OutboxEdit.body] goes in as a JSON string, as it is. */
    fun requestJson(edit: OutboxEdit): String =
        "{\"op_id\":${MiniJson.quote(edit.opId)},\"method\":${MiniJson.quote(edit.method)}," +
            "\"path\":${MiniJson.quote(edit.path)},\"body\":${MiniJson.quote(edit.body)},\"made_at\":${edit.madeAt}}"

    /**
     * The edit and state endpoints answer with JSON (their own errors too). Anything else, such as
     * a login page of a Wi-Fi hotspot, a proxy's error page or an old server without these routes,
     * is not an answer: a change must never be marked done or refused because of it.
     */
    fun accept(status: Int, contentType: String?, body: String): ServerReply {
        if (status == 0) return ServerReply.noAnswer("No answer from the server")
        // 401, 403 and 5xx decide nothing about the change itself, whatever they carry.
        if (status == 401 || status == 403 || status >= 500) return ServerReply(status, body)
        val json = contentType?.trim()?.lowercase()?.startsWith("application/json") == true
        if (!json) return ServerReply.noAnswer("The server did not answer as Nimrag (HTTP $status), check the server URL")
        return ServerReply(status, body)
    }

    /** Why a change stays on the phone, in words (no secrets). */
    fun keptReason(reply: ServerReply): String = reply.problem ?: when (reply.status) {
        0 -> "No answer from the server"
        401, 403 -> "The server no longer accepts this phone (HTTP ${reply.status})"
        408, 429 -> "HTTP ${reply.status}, will retry"
        in 500..599 -> "Server error (HTTP ${reply.status}), will retry"
        else -> "Unexpected answer (HTTP ${reply.status}), will retry"
    }

    fun tokenRejected(status: Int) = status == 401 || status == 403

    /** On app resume: pull again unless the last pull was just now. */
    fun resumePullDue(lastPullAtMs: Long, nowMs: Long): Boolean =
        lastPullAtMs <= 0 || nowMs - lastPullAtMs >= RESUME_PULL_GAP_MS || nowMs < lastPullAtMs
}

/**
 * Sends the outbox, oldest first, one change at a time. Applied and Dropped go on with the next
 * one; Kept stops the pass (the server is away or unwell, or refused the token).
 */
class EditDrain(private val outbox: EditOutbox) {
    sealed interface End {
        /** Nothing left to send. */
        data object Empty : End

        /** A change stays: back off like a file. */
        data class Kept(val reason: String) : End

        /** 401/403 from the endpoint itself: stop, like file uploads. */
        data class TokenRejected(val status: Int) : End
    }

    data class Report(val applied: Int, val dropped: Int, val end: End)

    /** [send] posts one change and returns the answer. */
    fun run(send: (OutboxEdit) -> ServerReply): Report {
        var applied = 0
        var dropped = 0
        var previous: String? = null
        while (true) {
            val edit = outbox.next() ?: return Report(applied, dropped, End.Empty)
            // The same change after it was reported done: a fault on the phone's side. Never loop on it.
            if (edit.opId == previous) {
                return Report(applied, dropped, End.Kept("The list of changes on the phone did not move on"))
            }
            previous = edit.opId
            val reply = send(edit)
            when (outbox.done(edit.opId, reply.status, reply.body)) {
                EditVerdict.Applied -> applied++
                EditVerdict.Dropped -> dropped++
                EditVerdict.Kept -> {
                    val end = if (EditPolicy.tokenRejected(reply.status)) {
                        End.TokenRejected(reply.status)
                    } else {
                        End.Kept(EditPolicy.keptReason(reply))
                    }
                    return Report(applied, dropped, end)
                }
            }
        }
    }
}

/**
 * Pulls the server's state onto the phone, only while the outbox is empty (a pull would undo
 * changes not yet sent).
 */
class StatePull(private val outbox: EditOutbox) {
    sealed interface Result {
        /** Changes wait: no request was made. */
        data object Skipped : Result

        /** The copy now carries the server's state. */
        data object Applied : Result

        /** A change arrived meanwhile: nothing changed, try again next time (not a failure). */
        data object Later : Result

        data class Failed(val reason: String) : Result

        data class TokenRejected(val status: Int) : Result
    }

    /** [fetch] is `GET /api/device/state`. */
    fun run(fetch: () -> ServerReply): Result {
        if (outbox.pending() > 0) return Result.Skipped
        val reply = fetch()
        if (EditPolicy.tokenRejected(reply.status)) return Result.TokenRejected(reply.status)
        if (reply.status != 200) return Result.Failed(EditPolicy.keptReason(reply))
        val applied = try {
            outbox.applyState(reply.body)
        } catch (_: Exception) {
            // The text may quote the page; keep only a fixed sentence.
            return Result.Failed("The server's state could not be read")
        }
        return if (applied) Result.Applied else Result.Later
    }
}

/** Back-off for the edits side of a pass (the outbox and the pull share it): 30 s doubling to 1 h. */
class EditBackoff {
    private var attempts = 0
    private var retryAt = 0L

    @Synchronized
    fun failed(nowMs: Long): Long {
        attempts++
        retryAt = UploadPolicy.nextAttemptAt(nowMs, attempts)
        return retryAt - nowMs
    }

    @Synchronized
    fun succeeded() {
        attempts = 0
        retryAt = 0L
    }

    /** 0 when a try is due now, else the milliseconds to wait. */
    @Synchronized
    fun waitMs(nowMs: Long): Long = if (retryAt > nowMs) retryAt - nowMs else 0L
}

/** What to do about a fresh count of waiting changes (from the poll while the app is on screen). */
object EditCountRule {
    data class Decision(val publish: Boolean, val wake: Boolean)

    /** [last] is null before the first look. A new change wakes the worker; any change updates the page. */
    fun decide(last: Int?, now: Int): Decision =
        Decision(publish = last != now, wake = now > 0 && (last == null || now > last))
}

/** The messages for changes the server refused. Shown once each. */
object DroppedEdits {
    /** What the change was, in a few plain words. Unknown requests get a general phrase. */
    fun what(method: String, path: String): String {
        val segments = path.substringBefore('?').trim('/').split('/')
        val m = method.uppercase()
        return when {
            segments == listOf("api", "settings") -> "a change to your settings"
            segments == listOf("api", "body") -> "a new weight or body entry"
            segments.size == 3 && segments[1] == "body" && m == "DELETE" -> "deleting a weight or body entry"
            segments.size == 4 && segments[1] == "activities" && segments[3] == "strength" -> "a change to a strength session"
            segments == listOf("api", "workouts") -> "a new workout"
            segments.size == 3 && segments[1] == "workouts" && m == "DELETE" -> "deleting a workout"
            segments.size == 3 && segments[1] == "workouts" -> "a change to a workout"
            segments.size == 4 && segments[1] == "workouts" && segments[3] == "send" && m == "DELETE" -> "taking a workout off the watch"
            segments.size == 4 && segments[1] == "workouts" && segments[3] == "send" -> "sending a workout to the watch"
            else -> "a change"
        }
    }

    /** The one line for the person; [DroppedChange.reason] is the server's own short sentence. */
    fun message(change: DroppedChange): String {
        val what = what(change.method, change.path)
        val reason = change.reason.trim().trimEnd('.')
        return if (reason.isEmpty()) {
            "A change made offline could not be saved: $what"
        } else {
            "A change made offline could not be saved: $what ($reason)"
        }
    }

    /** The log line: the kind of change and the server's reason, never the path (it may hold ids). */
    fun logLine(change: DroppedChange): String =
        "Dropped an offline change (${what(change.method, change.path)}): ${change.reason.trim().take(120)}"
}
