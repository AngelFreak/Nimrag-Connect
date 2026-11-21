package dev.nimrag.app.engine.workouts.logic

import dev.nimrag.app.engine.auth.logic.MiniJson
import uniffi.nimrag_ffi.UploadResult
import uniffi.nimrag_ffi.UploadStage

/**
 * The pure part of sending workouts to the watch (stage 4a): reading the server's queue,
 * choosing what to fetch, turning the protocol client's [UploadResult] into the report the
 * server shows its owner. No Android types, so the unit tests run on the JVM.
 *
 * The statuses named here are Gadgetbridge's names for the values the watch sends back;
 * *inferred (Gadgetbridge)* until a watch has refused something (docs/protocol/PROTOCOL.md).
 */
object WorkoutSendRules {
    /** A workout is file type 128 (FIT), sub type 5. */
    const val DATA_TYPE = 128
    const val SUB_TYPE = 5

    /** While the link is ready the queue is looked at this often (and right after the handshake). */
    const val POLL_INTERVAL_MS = 60_000L

    /** Workouts fetched per look: the client queues at most eight uploads, and one at a time is sent. */
    const val MAX_PER_CHECK = 4

    /** A workout file is a few kilobytes; anything near this is not one. */
    const val MAX_FIT_BYTES = 256 * 1024

    /** Longest reason that is sent to the server (it keeps 200 characters). */
    const val MAX_REASON_CHARS = 200

    /** One entry of `GET /api/device/workouts`. */
    data class Queued(val id: Long, val name: String, val revision: Long)

    /** The queue, or null when the text is not what the server sends. Malformed entries are dropped. */
    fun parseQueue(text: String): List<Queued>? {
        val root = MiniJson.parseObject(text) ?: return null
        val list = root["workouts"] as? List<*> ?: return null
        return list.mapNotNull { entry ->
            val m = entry as? Map<*, *> ?: return@mapNotNull null
            val id = (m["id"] as? Double)?.toLong() ?: return@mapNotNull null
            val revision = (m["revision"] as? Double)?.toLong() ?: return@mapNotNull null
            val name = m["name"] as? String ?: return@mapNotNull null
            if (id <= 0 || revision <= 0) null else Queued(id, name, revision)
        }
    }

    /** The workouts to fetch now: queue order, not already on their way, at most [max]. */
    fun pick(queue: List<Queued>, inFlight: Set<Long>, max: Int = MAX_PER_CHECK): List<Queued> =
        queue.filter { it.id !in inFlight }.take(max)

    /** The `error` text of an error answer (`{"error": "..."}`), or a fixed text. */
    fun errorText(body: ByteArray, fallback: String): String {
        val text = try {
            String(body, Charsets.UTF_8)
        } catch (_: Exception) {
            return fallback
        }
        val message = MiniJson.parseObject(text)?.get("error") as? String
        return (message ?: fallback).filter { !it.isISOControl() }.take(MAX_REASON_CHARS)
    }

    /** Body of `POST /api/device/workouts/{id}/result`. */
    fun resultBody(revision: Long, ok: Boolean, error: String?): String = buildString {
        append("{\"revision\":").append(revision).append(",\"ok\":").append(ok)
        if (!ok && error != null) append(",\"error\":").append(MiniJson.quote(error.take(MAX_REASON_CHARS)))
        append('}')
    }

    /** What to tell the server about one upload. */
    sealed interface Report {
        /** The watch took the file. */
        data object Sent : Report

        /** It did not work; [reason] is shown to the owner. */
        data class Failed(val reason: String) : Report

        /**
         * Nothing to report: the link closed before the upload ended, so the workout stays
         * queued and goes out on the next connection.
         */
        data object Keep : Report
    }

    fun reportFor(result: UploadResult): Report = when (result) {
        is UploadResult.Sent -> Report.Sent
        is UploadResult.Aborted -> Report.Keep
        else -> Report.Failed(describe(result))
    }

    /** A fixed, content-free sentence for a failed upload. */
    fun describe(result: UploadResult): String = when (result) {
        is UploadResult.Sent -> "Sent"
        is UploadResult.Refused ->
            "The watch refused the workout while ${stage(result.stage)} (${statusName(result.stage, result.status.toInt())})"
        is UploadResult.Nak ->
            "The watch answered with an error while ${stage(result.stage)} (status ${result.status})"
        is UploadResult.TimedOut -> "The watch did not answer while ${stage(result.stage)}"
        is UploadResult.BadReply -> "The watch's answer while ${stage(result.stage)} was not understood"
        is UploadResult.Aborted -> "The connection to the watch closed before the workout was sent"
        is UploadResult.Invalid -> "The workout could not be sent over this connection"
    }

    private fun stage(stage: UploadStage): String = when (stage) {
        UploadStage.CREATE -> "creating the file"
        UploadStage.REQUEST -> "starting the transfer"
        UploadStage.DATA -> "sending the data"
    }

    /** Gadgetbridge's names for the create (5005), upload (5003) and transfer (5004) statuses. */
    fun statusName(stage: UploadStage, status: Int): String {
        val names = when (stage) {
            UploadStage.CREATE -> listOf("OK", "DUPLICATE", "NO_SPACE", "UNSUPPORTED", "NO_SLOTS", "NO_SPACE_FOR_TYPE")
            UploadStage.REQUEST ->
                listOf("OK", "INDEX_UNKNOWN", "INDEX_NOT_WRITEABLE", "NO_SPACE_LEFT", "INVALID", "NOT_READY", "CRC_INCORRECT")
            UploadStage.DATA -> listOf("OK", "RESEND", "ABORT", "CRC_MISMATCH", "OFFSET_MISMATCH", "SYNC_PAUSED")
        }
        val name = names.getOrNull(status)
        return if (name != null) "status $status, $name" else "status $status"
    }
}
