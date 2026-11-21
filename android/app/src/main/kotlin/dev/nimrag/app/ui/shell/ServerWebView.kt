package dev.nimrag.app.ui.shell

import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.rememberLauncherForActivityResult
import android.annotation.SuppressLint
import android.app.AlertDialog
import android.app.DownloadManager
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.graphics.Bitmap
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.view.View
import android.widget.Toast
import android.webkit.CookieManager
import android.webkit.JavascriptInterface
import android.webkit.JsResult
import android.webkit.RenderProcessGoneDetail
import android.webkit.ServiceWorkerController
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.viewinterop.AndroidView
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import dev.nimrag.app.engine.local.LocalWeb
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.ui.shell.logic.BridgeScripts
import dev.nimrag.app.ui.shell.logic.BridgeStatus
import dev.nimrag.app.ui.shell.logic.DownloadPolicy
import dev.nimrag.app.ui.shell.logic.NavigationDecision
import dev.nimrag.app.ui.shell.logic.Origin
import dev.nimrag.app.ui.shell.logic.ServerOrigin
import dev.nimrag.app.ui.shell.logic.ServerTarget
import dev.nimrag.app.ui.theme.NimragColors
import java.util.concurrent.Callable
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit

/*
 * The one web view of the app (IMPLEMENTATION_PLAN.md, stage 3n). It loads the saved server
 * address and nothing else: other origins never load in it (web links go to the browser, other
 * schemes are dropped), no file or content access, no mixed content, Safe Browsing on. The
 * page talks to the app through [NimragWebBridge], whose methods act only while the web view's
 * current URL is on the server origin.
 */

/** What the shell knows about the web view; the web view itself is created by [ServerWebView]. */
@Stable
class WebShellState {
    /** The server cannot be reached and nothing is cached: the shell shows its retry overlay. */
    var unreachable by mutableStateOf(false)
        private set

    /** A retry is under way (the overlay shows progress). */
    var retrying by mutableStateOf(false)
        private set

    /** The web view has history to go back in. */
    var canGoBack by mutableStateOf(false)
        private set

    /** Bumped when the renderer died: the shell builds a new web view. */
    var generation by mutableIntStateOf(0)
        private set

    private var view: WebView? = null
    private var target: ServerTarget? = null
    private var pageReady = false
    private var errorThisLoad = false
    private var pendingPath: String? = null
    private var statusProvider: () -> BridgeStatus = { BridgeStatus.Unknown }
    private var released = false
    private var remote = true
    private var attachedEpoch = 0

    /** Called (main thread) when the real server's session is gone; the shell hands it to the account. */
    var sessionListener: (() -> Unit)? = null

    internal fun attach(
        webView: WebView,
        target: ServerTarget,
        statusProvider: () -> BridgeStatus,
        remote: Boolean,
        sessionEpoch: Int,
    ) {
        view = webView
        this.remote = remote
        attachedEpoch = sessionEpoch
        this.target = target
        this.statusProvider = statusProvider
        pageReady = false
        errorThisLoad = false
        pendingPath = null
        released = false
        unreachable = false
        retrying = false
        canGoBack = false
    }

    internal fun detach(webView: WebView) {
        if (view === webView) {
            view = null
            released = true
        }
    }

    /** A native tab was tapped: tell the page's router, or queue it until the page is up. */
    fun navigate(path: String) {
        if (unreachable) {
            // The page is not there to route; a tap is another try at reaching the server.
            pendingPath = path
            retry()
            return
        }
        if (pageReady) runInPage(BridgeScripts.navigateEvent(path)) else pendingPath = path
    }

    /**
     * The web app or the web view found the session gone (a 401). Only the real server's session
     * counts: the phone's own copy is signed in by the app itself.
     */
    internal fun sessionExpired() {
        if (remote && !released) sessionListener?.invoke()
    }

    /**
     * A new session cookie is in the jar (unlock): reload the page the web view is on, unless it
     * was created after that cookie went in. Main thread.
     */
    fun reloadForSession(epoch: Int) {
        if (epoch == attachedEpoch) return
        attachedEpoch = epoch
        val v = view ?: return
        val t = target ?: return
        if (!remote) return
        errorThisLoad = false
        if (v.url.isNullOrBlank()) v.loadUrl(t.startUrl) else v.reload()
    }

    /** Web history first; false when there is nothing to go back to. */
    fun goBack(): Boolean {
        val v = view ?: return false
        if (!v.canGoBack()) return false
        v.goBack()
        return true
    }

    fun retry() {
        val v = view ?: return
        val t = target ?: return
        retrying = true
        errorThisLoad = false
        v.loadUrl(t.startUrl)
    }

    /** Tells the page the link or last sync changed (the page re-reads `NimragApp.status()`). */
    fun dispatchStatus(status: BridgeStatus) {
        if (pageReady) runInPage(BridgeScripts.statusEvent(status.toJson()))
    }

    /** New watch data reached the server: the page fetches again (it debounces a burst). */
    fun dispatchData() {
        if (pageReady) runInPage(BridgeScripts.DATA_EVENT_SCRIPT)
    }

    fun onPause() {
        CookieManager.getInstance().flush()
        view?.onPause()
    }

    fun onResume() {
        view?.onResume()
        // Coming back after hours or days: the server may have a newer web app by now.
        if (pageReady) runInPage(BridgeScripts.UPDATE_CHECK_SCRIPT)
    }

    /** Runs [script] in the page, only when the web view is on the server origin. Main thread. */
    private fun runInPage(script: String) {
        val v = view ?: return
        if (released || !ServerOrigin.isSameOrigin(v.url, target?.origin)) return
        v.evaluateJavascript(script, null)
    }

    // ---- callbacks from the web view client (main thread)

    internal fun pageStarted(url: String?) {
        if (ServerOrigin.originOf(url) == null) return
        pageReady = false
        errorThisLoad = false
    }

    internal fun pageFinished(url: String?, view: WebView, documentStartSupported: Boolean) {
        canGoBack = view.canGoBack()
        if (ServerOrigin.originOf(url) == null || errorThisLoad) return
        unreachable = false
        retrying = false
        pageReady = true
        if (!documentStartSupported) injectFallback(view)
        pendingPath?.let {
            pendingPath = null
            runInPage(BridgeScripts.navigateEvent(it))
        }
        runInPage(BridgeScripts.statusEvent(statusProvider().toJson()))
        runInPage(BridgeScripts.UPDATE_CHECK_SCRIPT)
    }

    /** Fallback when DOCUMENT_START_SCRIPT is missing: define the bridge once the page is up. */
    internal fun injectFallback(view: WebView) {
        if (ServerOrigin.isSameOrigin(view.url, target?.origin)) view.evaluateJavascript(BridgeScripts.DEFINE_BRIDGE, null)
    }

    internal fun historyChanged(view: WebView) {
        canGoBack = view.canGoBack()
    }

    internal fun loadFailed() {
        errorThisLoad = true
        pageReady = false
        retrying = false
        unreachable = true
    }

    internal fun rendererGone() {
        pageReady = false
        generation++
    }
}

/** Plain remembered holder so a screen can drive the web view. */
@Composable
fun rememberWebShellState(): WebShellState = remember { WebShellState() }

/**
 * Creates the web view once per [target] (and per renderer restart) and keeps it alive while it
 * is composed, also when [visible] is false (another tab is on screen).
 */
@Composable
fun ServerWebView(
    target: ServerTarget,
    localWeb: LocalWeb?,
    state: WebShellState,
    controller: WatchController,
    sessionEpoch: Int,
    visible: Boolean,
    onOpenWatch: () -> Unit,
    onRoute: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val openWatch by rememberUpdatedState(onOpenWatch)
    val route by rememberUpdatedState(onRoute)
    // <input type="file"> (e.g. the Garmin token import): the system picker, one request at a time.
    val pendingFiles = remember { arrayOfNulls<ValueCallback<Array<Uri>>>(1) }
    val filePicker = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        pendingFiles[0]?.onReceiveValue(WebChromeClient.FileChooserParams.parseResult(result.resultCode, result.data))
        pendingFiles[0] = null
    }
    val chooseFile: (ValueCallback<Array<Uri>>, Intent) -> Boolean = { callback, intent ->
        pendingFiles[0]?.onReceiveValue(null)
        pendingFiles[0] = callback
        try {
            filePicker.launch(intent)
            true
        } catch (e: ActivityNotFoundException) {
            pendingFiles[0] = null
            false
        }
    }
    androidx.compose.runtime.key(target.startUrl, state.generation) {
        AndroidView(
            factory = { context ->
                createWebView(
                    context = context,
                    target = target,
                    localWeb = localWeb,
                    state = state,
                    controller = controller,
                    sessionEpoch = sessionEpoch,
                    onOpenWatch = { openWatch() },
                    onRoute = { route(it) },
                    chooseFile = chooseFile,
                )
            },
            modifier = modifier,
            update = { it.visibility = if (visible && !state.unreachable) View.VISIBLE else View.INVISIBLE },
            onRelease = { webView ->
                state.detach(webView)
                (webView.tag as? Runnable)?.run()
                webView.destroy()
            },
        )
    }
}

@SuppressLint("SetJavaScriptEnabled")
private fun createWebView(
    context: Context,
    target: ServerTarget,
    localWeb: LocalWeb?,
    state: WebShellState,
    controller: WatchController,
    sessionEpoch: Int,
    onOpenWatch: () -> Unit,
    onRoute: (String) -> Unit,
    chooseFile: (ValueCallback<Array<Uri>>, Intent) -> Boolean,
): WebView {
    val webView = WebView(context)
    webView.setBackgroundColor(NimragColors.Background.toArgb())

    if (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) {
        WebView.setWebContentsDebuggingEnabled(true)
    }

    webView.settings.apply {
        javaScriptEnabled = true
        domStorageEnabled = true // the PWA's storage
        allowFileAccess = false
        allowContentAccess = false
        mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
        safeBrowsingEnabled = true
        javaScriptCanOpenWindowsAutomatically = false
        setSupportMultipleWindows(false)
        setGeolocationEnabled(false)
        // The local server is on the phone itself: never stale, never "offline".
        cacheMode = if (localWeb != null) WebSettings.LOAD_DEFAULT else cacheModeFor(context)
    }
    // Service workers stay on (the PWA's offline pages); they get no file or content access either.
    ServiceWorkerController.getInstance().serviceWorkerWebSettings.apply {
        allowFileAccess = false
        allowContentAccess = false
    }
    // Cookies persist (the session login lives in the web view); first-party only.
    CookieManager.getInstance().apply {
        setAcceptCookie(true)
        setAcceptThirdPartyCookies(webView, false)
    }

    // Offline: with no network the HTTP cache answers first; with one, the normal rules apply.
    val stopWatchingNetwork = if (localWeb != null) NoStop else watchNetwork(context) { online ->
        webView.settings.cacheMode = if (online) WebSettings.LOAD_DEFAULT else WebSettings.LOAD_CACHE_ELSE_NETWORK
    }
    webView.tag = Runnable { stopWatchingNetwork() }

    val statusProvider = { BridgeStatus.of(controller.state.value) }
    state.attach(webView, target, statusProvider, remote = localWeb == null, sessionEpoch = sessionEpoch)

    webView.addJavascriptInterface(
        NimragWebBridge(webView, target.origin, controller, onOpenWatch, onRoute, state::sessionExpired),
        BridgeScripts.INTERFACE_NAME,
    )
    // window.NimragApp exists before the page's own scripts run, on the server origin only. Where
    // the web view lacks the feature, the script is injected when the page starts and finishes
    // instead (a page that reads NimragApp very early can then miss it).
    var documentStart = false
    if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
        WebViewCompat.addDocumentStartJavaScript(webView, BridgeScripts.DEFINE_BRIDGE, setOf(target.origin.rule))
        documentStart = true
    }

    // A web view saves no file by itself. "Download my data" (a link to /api/export) ends here.
    webView.setDownloadListener { url, userAgent, contentDisposition, mimeType, _ ->
        startDownload(context, target.origin, url, userAgent, contentDisposition, mimeType)
    }
    webView.webViewClient = ShellWebViewClient(target.origin, state, documentStart)
    webView.webChromeClient = ShellChromeClient(chooseFile)
    if (localWeb != null) {
        // The launch secret goes into the cookie jar before the first request; the local server
        // answers 403 to anything without it (other apps can reach 127.0.0.1 too).
        CookieManager.getInstance().setCookie(target.startUrl, localWeb.cookie()) { webView.loadUrl(target.startUrl) }
    } else {
        webView.loadUrl(target.startUrl)
    }
    return webView
}

/**
 * Hands a download of the server's own origin to Android's DownloadManager, which saves it to the
 * Downloads folder and shows its own progress and "done" notification. The request carries the web
 * view's cookies for that URL (the session), so it is signed in as the page was. Anything from
 * another origin is dropped: the cookie goes nowhere else.
 */
private fun startDownload(
    context: Context,
    origin: Origin,
    url: String?,
    userAgent: String?,
    contentDisposition: String?,
    mimeType: String?,
) {
    if (url == null || !DownloadPolicy.allow(url, origin)) return
    val name = DownloadPolicy.fileName(contentDisposition, url)
    try {
        val request = DownloadManager.Request(Uri.parse(url))
            .setTitle(name)
            .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
            .setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, name)
        CookieManager.getInstance().getCookie(url)?.let { request.addRequestHeader("Cookie", it) }
        userAgent?.let { request.addRequestHeader("User-Agent", it) }
        if (!mimeType.isNullOrBlank()) request.setMimeType(mimeType)
        context.getSystemService(DownloadManager::class.java).enqueue(request)
        Toast.makeText(context, "Downloading $name", Toast.LENGTH_SHORT).show()
    } catch (e: Exception) {
        // No download manager, or the system refused the request.
        Toast.makeText(context, "Could not start the download", Toast.LENGTH_LONG).show()
    }
}

private fun cacheModeFor(context: Context): Int =
    if (isOnline(context)) WebSettings.LOAD_DEFAULT else WebSettings.LOAD_CACHE_ELSE_NETWORK

internal fun isOnline(context: Context): Boolean {
    val cm = context.getSystemService(ConnectivityManager::class.java) ?: return true
    val caps = cm.getNetworkCapabilities(cm.activeNetwork ?: return false) ?: return false
    return caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
}

/** Calls [onChange] on the main thread when the default network appears or goes; returns the stop function. */
private fun watchNetwork(context: Context, onChange: (online: Boolean) -> Unit): () -> Unit {
    val cm = context.getSystemService(ConnectivityManager::class.java) ?: return {}
    val main = Handler(Looper.getMainLooper())
    val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            main.post { onChange(true) }
        }

        override fun onLost(network: Network) {
            main.post { onChange(false) }
        }
    }
    val stop: () -> Unit = { runCatching { cm.unregisterNetworkCallback(callback) } }
    return try {
        cm.registerDefaultNetworkCallback(callback)
        stop
    } catch (_: SecurityException) {
        NoStop
    }
}

private val NoStop: () -> Unit = {}

private class ShellWebViewClient(
    private val origin: Origin,
    private val state: WebShellState,
    private val documentStartSupported: Boolean,
) : WebViewClient() {
    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
        return when (ServerOrigin.decide(request.url.toString(), origin, request.isForMainFrame)) {
            NavigationDecision.Allow -> false
            NavigationDecision.OpenExternal -> {
                openInBrowser(view.context, request.url)
                true
            }
            NavigationDecision.Block -> true
        }
    }

    override fun onPageStarted(view: WebView, url: String?, favicon: Bitmap?) {
        state.pageStarted(url)
        if (!documentStartSupported) state.injectFallback(view)
    }

    override fun onPageFinished(view: WebView, url: String?) {
        state.pageFinished(url, view, documentStartSupported)
    }

    override fun doUpdateVisitedHistory(view: WebView, url: String?, isReload: Boolean) {
        state.historyChanged(view)
    }

    override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
        if (!request.isForMainFrame) return
        // A navigation that was replaced by another one is not a failure.
        if (error.description?.contains("ERR_ABORTED") == true) return
        state.loadFailed()
    }

    override fun onReceivedHttpError(view: WebView, request: WebResourceRequest, errorResponse: WebResourceResponse) {
        // A reverse proxy answers 502 to 504 when the app behind it is down.
        if (request.isForMainFrame && errorResponse.statusCode in 502..504) state.loadFailed()
        // A 401 from the server's own API: the session is gone (a second signal next to the web
        // app's own call, for a page whose scripts did not get that far). The shell only acts on
        // it while signed in, so the web app's own "who am I" probe cannot loop.
        if (errorResponse.statusCode == 401 &&
            ServerOrigin.decide(request.url.toString(), origin, false) == NavigationDecision.Allow &&
            request.url.path?.startsWith("/api/") == true
        ) {
            state.sessionExpired()
        }
    }

    override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
        state.rendererGone()
        return true
    }
}

private fun openInBrowser(context: Context, uri: Uri) {
    val intent = Intent(Intent.ACTION_VIEW, uri).addCategory(Intent.CATEGORY_BROWSABLE)
    try {
        context.startActivity(intent)
    } catch (_: ActivityNotFoundException) {
        // No browser installed: nothing to do.
    }
}

/** alert() and confirm() from the page (without a chrome client they are silently dropped). */
private class ShellChromeClient(
    private val chooseFile: (ValueCallback<Array<Uri>>, Intent) -> Boolean,
) : WebChromeClient() {
    override fun onShowFileChooser(
        view: WebView,
        filePathCallback: ValueCallback<Array<Uri>>,
        params: FileChooserParams,
    ): Boolean = chooseFile(filePathCallback, params.createIntent())

    override fun onJsAlert(view: WebView, url: String?, message: String?, result: JsResult): Boolean {
        AlertDialog.Builder(view.context)
            .setMessage(message)
            .setPositiveButton(android.R.string.ok) { _, _ -> result.confirm() }
            .setOnCancelListener { result.cancel() }
            .show()
        return true
    }

    override fun onJsConfirm(view: WebView, url: String?, message: String?, result: JsResult): Boolean {
        AlertDialog.Builder(view.context)
            .setMessage(message)
            .setPositiveButton(android.R.string.ok) { _, _ -> result.confirm() }
            .setNegativeButton(android.R.string.cancel) { _, _ -> result.cancel() }
            .setOnCancelListener { result.cancel() }
            .show()
        return true
    }
}

/**
 * `window.NimragAppNative`. The web view calls these on its own "JavaBridge" thread; every one
 * hops to the main thread, where it checks that the web view's current URL is on the server
 * origin before it does anything. [status] blocks the caller for that hop (2 s at most).
 *
 * Note: addJavascriptInterface exposes the object to every frame, including frames on other
 * origins; the check uses the top-level URL, and the shell drops other-origin frame navigations.
 */
internal class NimragWebBridge(
    private val webView: WebView,
    private val origin: Origin,
    private val controller: WatchController,
    private val onOpenWatch: () -> Unit,
    private val onRoute: (String) -> Unit,
    private val onSessionExpired: () -> Unit,
) {
    private val main = Handler(Looper.getMainLooper())

    /** Main thread only: `WebView.getUrl()` is not thread safe. */
    private fun trusted(): Boolean = ServerOrigin.isSameOrigin(webView.url, origin)

    @JavascriptInterface
    fun syncNow() {
        main.post { if (trusted()) controller.syncNow() }
    }

    @JavascriptInterface
    fun status(): String = onMain(BridgeStatus.Unknown.toJson()) {
        if (trusted()) BridgeStatus.of(controller.state.value).toJson() else BridgeStatus.Unknown.toJson()
    }

    /** The web app got a 401 and shows no login page of its own: the app signs in again. */
    @JavascriptInterface
    fun sessionExpired() {
        main.post { if (trusted()) onSessionExpired() }
    }

    @JavascriptInterface
    fun openWatch() {
        main.post { if (trusted()) onOpenWatch() }
    }

    @JavascriptInterface
    fun onRoute(path: String?) {
        if (path == null || path.length > MAX_ROUTE_LENGTH) return
        main.post { if (trusted()) onRoute.invoke(path) }
    }

    private fun <T> onMain(fallback: T, block: () -> T): T {
        if (Looper.myLooper() == Looper.getMainLooper()) return block()
        val task = FutureTask(Callable { block() })
        main.post(task)
        return try {
            task.get(MAIN_WAIT_SECONDS, TimeUnit.SECONDS)
        } catch (_: Exception) {
            task.cancel(false)
            fallback
        }
    }

    private companion object {
        const val MAX_ROUTE_LENGTH = 2048
        const val MAIN_WAIT_SECONDS = 2L
    }
}
