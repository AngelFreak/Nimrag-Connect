package dev.nimrag.app.ui.shell.logic

import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.WatchState
import java.time.Instant
import java.time.temporal.ChronoUnit

/** The `link` values of `NimragApp.status()`; [wire] is what the web app sees (web/src/native.ts). */
enum class BridgeLink(val wire: String) {
    Connected("connected"),
    Waiting("waiting"),
    Problem("problem"),
    Unknown("unknown"),
}

/**
 * What the web app is told about the watch: the link and the last finished sync. A data class so
 * the shell can dispatch `nimrag:status` exactly when it changes.
 */
data class BridgeStatus(val link: BridgeLink, val lastSyncAtMs: Long?) {
    /** `{"link":"connected","lastSyncAt":"2026-11-19T10:00:00Z"}`, `lastSyncAt` null when never. */
    fun toJson(): String {
        val at = lastSyncAtMs?.takeIf { it > 0 }?.let { Instant.ofEpochMilli(it).truncatedTo(ChronoUnit.SECONDS).toString() }
        return "{\"link\":\"${link.wire}\",\"lastSyncAt\":${if (at == null) "null" else "\"$at\""}}"
    }

    companion object {
        val Unknown = BridgeStatus(BridgeLink.Unknown, null)

        fun of(state: AppState): BridgeStatus = BridgeStatus(linkOf(state), state.sync.lastResult?.finishedAtMs)

        /**
         * No watch is Unknown (nothing to be wrong), pairing is Waiting. For a remembered watch:
         * Ready is Connected; Waiting and Connecting are Waiting; an offline engine, a lost
         * pairing, a model that cannot sync, or Bluetooth permission or adapter off are Problem.
         */
        fun linkOf(state: AppState): BridgeLink = when (val watch = state.watch) {
            WatchState.NotPaired -> BridgeLink.Unknown
            is WatchState.Pairing -> BridgeLink.Waiting
            is WatchState.Paired -> when {
                !state.permissions.bluetooth || !state.permissions.bluetoothEnabled -> BridgeLink.Problem
                watch.unsupportedReason != null -> BridgeLink.Problem
                else -> when (watch.link) {
                    LinkState.Ready -> BridgeLink.Connected
                    LinkState.Waiting, LinkState.Connecting -> BridgeLink.Waiting
                    LinkState.Offline, LinkState.BondLost -> BridgeLink.Problem
                }
            }
        }
    }
}

/** The JavaScript the shell runs in the page. Pure strings, so the tests can read them. */
object BridgeScripts {
    /** Name of the Java object added with addJavascriptInterface. */
    const val INTERFACE_NAME = "NimragAppNative"

    /**
     * Defines `window.NimragApp` for the web app (web/src/native.ts). The native object is looked
     * up at call time, so the script does not depend on it being injected first. Safe to run twice.
     */
    val DEFINE_BRIDGE: String = """
        (function () {
          if (window.NimragApp) return;
          var UNKNOWN = { link: 'unknown', lastSyncAt: null };
          function bridge() { return window.$INTERFACE_NAME; }
          window.NimragApp = {
            native: true,
            syncNow: function () { var n = bridge(); if (n) n.syncNow(); },
            status: function () {
              try {
                var n = bridge();
                var s = n ? JSON.parse(n.status()) : null;
                return s && typeof s === 'object' ? s : UNKNOWN;
              } catch (e) { return UNKNOWN; }
            },
            openWatch: function () { var n = bridge(); if (n) n.openWatch(); },
            sessionExpired: function () { var n = bridge(); if (n) n.sessionExpired(); },
            onRoute: function (path) { var n = bridge(); if (n) n.onRoute(String(path)); }
          };
        })();
    """.trimIndent()

    /** Tells the page the link or last sync changed; it re-reads `NimragApp.status()`. */
    fun statusEvent(statusJson: String): String =
        "window.dispatchEvent(new CustomEvent('nimrag:status',{detail:$statusJson}));"

    /** Tells the page's router to go to [path] (a native tab was tapped). */
    fun navigateEvent(path: String): String =
        "window.dispatchEvent(new CustomEvent('nimrag:navigate',{detail:{path:${jsString(path)}}}));"

    /** A JavaScript string literal: quoted, with quotes, backslashes, newlines and `<` escaped. */
    fun jsString(value: String): String = buildString {
        append('"')
        for (c in value) {
            when (c) {
                '"' -> append("\\\"")
                '\\' -> append("\\\\")
                '\n' -> append("\\n")
                '\r' -> append("\\r")
                ' ' -> append("\\u2028")
                ' ' -> append("\\u2029")
                '<' -> append("\\u003c")
                else -> if (c < ' ') append("\\u%04x".format(c.code)) else append(c)
            }
        }
        append('"')
    }
}
