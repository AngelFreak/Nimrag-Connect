package dev.nimrag.app.ui.shell.logic

import dev.nimrag.app.model.WebMode

/** Which server the web view shows. */
enum class WebSource { Remote, Local }

/**
 * Pure rule for the web view's source (no Android types).
 *
 * The real server is the source of truth (history, edits, desktop parity), so it wins whenever
 * it is reachable and has everything. The phone's own copy (the local server) is used when it is
 * the better or the only choice: no network, the server failed to answer this session, or
 * uploads are still pending (the phone holds data the server does not have yet). The choice is
 * made when the web view is (re)created, never while a page is open.
 */
object WebSourcePolicy {
    fun choose(
        mode: WebMode,
        localReady: Boolean,
        remoteConfigured: Boolean,
        online: Boolean,
        remoteFailed: Boolean,
        pendingUploads: Int,
    ): WebSource {
        if (!localReady) return WebSource.Remote
        if (!remoteConfigured) return WebSource.Local
        return when (mode) {
            WebMode.Local -> WebSource.Local
            WebMode.Remote -> WebSource.Remote
            WebMode.Auto -> if (!online || remoteFailed || pendingUploads > 0) WebSource.Local else WebSource.Remote
        }
    }
}
