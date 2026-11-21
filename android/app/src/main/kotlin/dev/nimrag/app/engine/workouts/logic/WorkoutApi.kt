package dev.nimrag.app.engine.workouts.logic

/** One HTTP answer: the status, the `X-Nimrag-Revision` header if there was one, the body. */
class HttpReply(val status: Int, val revisionHeader: String?, val body: ByteArray)

/** The two calls the workout API needs; the real one is [dev.nimrag.app.engine.workouts.UrlConnectionHttp]. */
interface WorkoutHttp {
    /** `GET` with the Bearer token; the body is cut at [maxBytes] (a longer answer is an IO error). */
    fun get(url: String, token: String, maxBytes: Int): HttpReply

    /** `POST` a JSON body with the Bearer token. */
    fun post(url: String, token: String, json: String): HttpReply
}

sealed interface QueueFetch {
    data class Ok(val workouts: List<WorkoutSendRules.Queued>) : QueueFetch

    /** The server answered, but not with a queue (`status` is what it said; 0 for text that was not a queue). */
    data class Failed(val status: Int) : QueueFetch
}

sealed interface FitFetch {
    class Ok(val bytes: ByteArray, val revision: Long) : FitFetch

    /** The server refused to make the file; [message] is its own text (for example "built without the FIT profile"). */
    data class Refused(val status: Int, val message: String) : FitFetch

    /** The workout is no longer queued (edited into another queue state, cancelled or deleted). */
    data object Gone : FitFetch
}

/**
 * The phone's side of the workout API (`/api/device/workouts`, Bearer token, no cookie). IO
 * errors are thrown for the caller to describe; the token only ever goes into a header.
 */
class WorkoutApi(private val http: WorkoutHttp, private val base: String, private val token: String) {
    fun queue(): QueueFetch {
        val reply = http.get("$base/api/device/workouts", token, MAX_JSON)
        if (reply.status != 200) return QueueFetch.Failed(reply.status)
        val list = WorkoutSendRules.parseQueue(String(reply.body, Charsets.UTF_8))
            ?: return QueueFetch.Failed(0)
        return QueueFetch.Ok(list)
    }

    fun fit(workout: WorkoutSendRules.Queued): FitFetch {
        val reply = http.get("$base/api/device/workouts/${workout.id}/fit", token, WorkoutSendRules.MAX_FIT_BYTES)
        return when {
            reply.status == 200 && reply.body.isNotEmpty() ->
                FitFetch.Ok(reply.body, reply.revisionHeader?.trim()?.toLongOrNull()?.takeIf { it > 0 } ?: workout.revision)
            reply.status == 404 -> FitFetch.Gone
            else -> FitFetch.Refused(
                reply.status,
                WorkoutSendRules.errorText(reply.body, "The server answered HTTP ${reply.status}"),
            )
        }
    }

    /** True when the server took the report (whether it applied it or not: a stale one is fine). */
    fun report(id: Long, revision: Long, ok: Boolean, error: String?): Boolean {
        val reply = http.post(
            "$base/api/device/workouts/$id/result",
            token,
            WorkoutSendRules.resultBody(revision, ok, error),
        )
        // A workout that is gone (404) will not be asked for again: nothing to retry.
        return reply.status == 200 || reply.status == 404
    }

    private companion object {
        const val MAX_JSON = 64 * 1024
    }
}
