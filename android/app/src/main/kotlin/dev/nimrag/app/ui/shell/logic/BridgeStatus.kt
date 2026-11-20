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
 * What the web app is told about the watch: the link, the last finished sync and whether a pull
 * from the watch is running (the web app spins its sync icon meanwhile). A data class so the shell
 * can dispatch `nimrag:status` exactly when it changes.
 */
data class BridgeStatus(val link: BridgeLink, val lastSyncAtMs: Long?, val syncing: Boolean = false) {
    /** `{"link":"connected","lastSyncAt":"2026-11-19T10:00:00Z","syncing":false}`, `lastSyncAt` null when never. */
    fun toJson(): String {
        val at = lastSyncAtMs?.takeIf { it > 0 }?.let { Instant.ofEpochMilli(it).truncatedTo(ChronoUnit.SECONDS).toString() }
        return "{\"link\":\"${link.wire}\",\"lastSyncAt\":${if (at == null) "null" else "\"$at\""},\"syncing\":$syncing}"
    }

    companion object {
        val Unknown = BridgeStatus(BridgeLink.Unknown, null)

        fun of(state: AppState): BridgeStatus =
            BridgeStatus(linkOf(state), state.sync.lastResult?.finishedAtMs, syncing = state.sync.running)

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
          var UNKNOWN = { link: 'unknown', lastSyncAt: null, syncing: false };
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

    /** Tells the page the link, last sync or `syncing` changed; it re-reads `NimragApp.status()`. */
    fun statusEvent(statusJson: String): String =
        "window.dispatchEvent(new CustomEvent('nimrag:status',{detail:$statusJson}));"

    /** Tells the page new watch data reached the server; it fetches what it shows again (web/src/dataRefresh.ts). */
    const val DATA_EVENT_SCRIPT = "window.dispatchEvent(new CustomEvent('nimrag:data'));"

    /**
     * Loads the server's newest web app when the page runs an older one. The app process lives on
     * (the watch service), so the web view can keep a page, and its service worker a cached copy,
     * from before a server update for days. Asks the server for its index page past the service
     * worker (a query it has not cached), compares the main script with the running one and, when
     * they differ, drops the service worker and its caches and reloads. Failures are ignored.
     */
    const val UPDATE_CHECK_SCRIPT = "(async()=>{try{" +
        "const cur=[...document.scripts].map(s=>s.src).find(s=>/\\/assets\\/index-[^/]+\\.js$/.test(s));if(!cur)return;" +
        "const r=await fetch('/?nimrag_build='+Date.now(),{cache:'no-store',credentials:'same-origin'});if(!r.ok)return;" +
        "const m=(await r.text()).match(/\\/assets\\/index-[^\"'/]+\\.js/);if(!m||cur.endsWith(m[0]))return;" +
        "if(navigator.serviceWorker){for(const g of await navigator.serviceWorker.getRegistrations())await g.unregister();}" +
        "if(window.caches){for(const k of await caches.keys())await caches.delete(k);}" +
        "location.reload();}catch(e){}})();"

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
