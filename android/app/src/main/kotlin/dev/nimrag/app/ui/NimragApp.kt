package dev.nimrag.app.ui

import androidx.activity.compose.BackHandler
import androidx.annotation.StringRes
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.NavigationRailItemDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import dev.nimrag.app.R
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.ui.components.ActivitiesIcon
import dev.nimrag.app.ui.components.HealthIcon
import dev.nimrag.app.ui.components.LocalNowMs
import dev.nimrag.app.ui.components.TodayIcon
import dev.nimrag.app.ui.components.WatchIcon
import dev.nimrag.app.ui.shell.CheckingScreen
import dev.nimrag.app.ui.shell.SignInScreen
import dev.nimrag.app.ui.shell.UnlockScreen
import dev.nimrag.app.ui.shell.ServerUnreachableScreen
import dev.nimrag.app.ui.shell.ServerWebView
import dev.nimrag.app.ui.shell.WebPreviewPlaceholder
import dev.nimrag.app.ui.shell.logic.BridgeStatus
import dev.nimrag.app.ui.shell.logic.MainTab
import dev.nimrag.app.ui.shell.logic.RouteTabs
import dev.nimrag.app.ui.shell.logic.NavStyle
import dev.nimrag.app.ui.shell.logic.ServerOrigin
import dev.nimrag.app.ui.shell.logic.ShellChrome
import dev.nimrag.app.ui.shell.logic.ShellNav
import dev.nimrag.app.ui.shell.logic.WebSource
import dev.nimrag.app.ui.shell.logic.WebSourcePolicy
import dev.nimrag.app.ui.shell.LocalCopyBanner
import dev.nimrag.app.ui.shell.isOnline
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.LocalContext
import dev.nimrag.app.ui.shell.rememberWebShellState
import dev.nimrag.app.ui.theme.NimragColors
import dev.nimrag.app.ui.theme.NimragTheme
import dev.nimrag.app.ui.watch.WatchTab
import dev.nimrag.app.ui.watch.logic.WatchPage
import dev.nimrag.app.ui.watch.logic.WatchProblems
import kotlinx.coroutines.delay

private val NavSaver = listSaver<ShellNav, Any>(
    save = { listOf(it.webTab.ordinal, it.inWatch) },
    restore = { ShellNav(MainTab.entries[it[0] as Int], it[1] as Boolean) },
)

/**
 * The whole app: theme, the native bottom bar (Today, Activities, Health, Watch), one
 * shared web view for the first three and the native Watch tab (one page and its sub-pages).
 * [demo] marks the fake controller.
 */
@Composable
fun NimragApp(controller: WatchController, demo: Boolean = false) {
    val state by controller.state.collectAsStateWithLifecycle()
    NimragTheme {
        NimragShell(state = state, controller = controller, demo = demo)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun NimragShell(
    state: AppState,
    controller: WatchController,
    demo: Boolean = false,
    initialPage: WatchPage = WatchPage.Main,
    startInWatch: Boolean = false,
) {
    var nav by rememberSaveable(stateSaver = NavSaver) { mutableStateOf(ShellNav(inWatch = startInWatch)) }
    // The Watch tab's page (an enum is Serializable, so it survives rotation as it is).
    var watchPage by rememberSaveable { mutableStateOf(initialPage) }

    val rawServerUrl = state.settings.serverUrl
    val remoteTarget = remember(rawServerUrl) { ServerOrigin.parse(rawServerUrl) }
    val webState = rememberWebShellState()
    val inspection = LocalInspectionMode.current
    val context = LocalContext.current

    // The phone's own copy of the data (the local server): used when the server is not the better
    // choice (see WebSourcePolicy). Decided when the web view is created; it changes while a page is
    // open only when uploads get stuck (switchToLocalNow) or on resume (backToServerOnResume).
    val localWeb = remember(state.localWebPort) { if (state.localWebPort != null) controller.localWeb() else null }
    var remoteFailed by remember { mutableStateOf(false) }
    var forceRemote by remember { mutableStateOf(false) }
    // Bumped to make the source be chosen again (see backToServerOnResume).
    var sourceCheck by remember { mutableIntStateOf(0) }

    // The account decides whether the server's web app may be shown at all: without a live session
    // the answer is the native sign-in or unlock screen, never the web app's own login page.
    val account by controller.account.state.collectAsStateWithLifecycle()
    val source = remember(localWeb, rawServerUrl, remoteFailed, forceRemote, account.phase, sourceCheck) {
        if (forceRemote) {
            WebSourcePolicy.remoteFor(account.phase)
        } else {
            WebSourcePolicy.choose(
                mode = controller.webMode(),
                localReady = localWeb != null,
                remoteConfigured = remoteTarget != null,
                online = isOnline(context),
                remoteFailed = remoteFailed,
                pendingUploads = state.uploads.pending,
                auth = account.phase,
            )
        }
    }
    val onLocal = source == WebSource.Local && localWeb != null
    val target = if (onLocal) ServerOrigin.parse(localWeb.startUrl) else remoteTarget
    // The server's web view stays alive (hidden) while the session is locked, so unlocking brings
    // the person back to the page they were on; signing out or the first check drops it.
    var remoteWebAlive by remember { mutableStateOf(false) }
    LaunchedEffect(source) {
        when (source) {
            WebSource.Remote -> remoteWebAlive = true
            WebSource.SignIn, WebSource.Checking -> remoteWebAlive = false
            else -> Unit
        }
    }
    val webShown = target != null && !inspection && when (source) {
        WebSource.Remote, WebSource.Local -> true
        WebSource.Unlock -> remoteWebAlive
        WebSource.SignIn, WebSource.Checking -> false
    }
    val webVisible = !nav.inWatch && (source == WebSource.Remote || source == WebSource.Local)
    // The web app (or the web view) found the session gone: the account locks or signs out.
    DisposableEffect(webState, controller) {
        webState.sessionListener = { controller.account.sessionExpired() }
        onDispose { webState.sessionListener = null }
    }
    // A new session cookie went in (unlock): the web view reloads with it.
    LaunchedEffect(account.sessionEpoch) { webState.reloadForSession(account.sessionEpoch) }
    // The server did not answer and the phone has a copy: switch to it for the rest of this run.
    LaunchedEffect(webState.unreachable, localWeb) {
        if (webState.unreachable && localWeb != null && !onLocal && !forceRemote) remoteFailed = true
    }
    // Files wait on the phone and uploads fail: the server's page cannot show them (and the
    // service worker keeps that page loading, so `unreachable` never comes). Switch now.
    val uploadsStuck = state.uploads.lastError != null || state.uploads.waitingForNetwork
    LaunchedEffect(uploadsStuck, state.uploads.pending > 0, onLocal) {
        if (WebSourcePolicy.switchToLocalNow(onLocal, localWeb != null, forceRemote, state.uploads.pending, uploadsStuck)) {
            remoteFailed = true
        }
    }

    // Permissions and Bluetooth can change while we are away (system settings, quick tiles).
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        controller.refreshPermissions()
        // Newest data first: sync when the last sync is old enough (the link must be ready).
        controller.appOpened()
        // Is the session still good? (Throttled in the account; the first check runs at start.)
        controller.account.checkSession()
        webState.onResume()
        // Everything reached the server again: it has the whole history, so it shows again.
        if (WebSourcePolicy.backToServerOnResume(onLocal, remoteTarget != null, isOnline(context), state.uploads.pending, uploadsStuck)) {
            remoteFailed = false
            forceRemote = false
            sourceCheck++
        }
    }
    LifecycleEventEffect(Lifecycle.Event.ON_PAUSE) { webState.onPause() }

    // The page learns about link and last-sync changes (and only changes).
    val bridgeStatus = BridgeStatus.of(state)
    LaunchedEffect(bridgeStatus) { webState.dispatchStatus(bridgeStatus) }

    // New files reached the server (or the phone's own copy): the page fetches again.
    LaunchedEffect(state.dataVersion) { if (state.dataVersion > 0) webState.dispatchData() }

    // Back: web history first; in the Watch tab its sub-pages first, then the web tabs; then exit.
    val backEnabled = if (nav.inWatch) watchPage != WatchPage.Main || target != null else webState.canGoBack
    BackHandler(enabled = backEnabled) {
        when {
            !nav.inWatch -> webState.goBack()
            watchPage != WatchPage.Main -> watchPage = watchPage.parent ?: WatchPage.Main
            else -> nav = nav.tapped(nav.webTab)
        }
    }

    // Relative times ("5 min ago") tick while the app is on screen.
    val nowMs = remember { mutableLongStateOf(System.currentTimeMillis()) }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                nowMs.longValue = System.currentTimeMillis()
                delay(30_000)
            }
        }
    }

    val imeVisible = WindowInsets.isImeVisible
    // The tab bar: hidden on the full-screen account steps, a side rail in landscape (short windows).
    val density = LocalDensity.current
    val windowHeightDp = with(density) { LocalWindowInfo.current.containerSize.height.toDp().value.toInt() }
    val navStyle = ShellChrome.navStyle(
        source = source,
        hasTarget = target != null,
        inWatch = nav.inWatch,
        windowHeightDp = windowHeightDp,
        imeVisible = imeVisible,
    )
    val onTab: (MainTab) -> Unit = { tab ->
        nav = nav.tapped(tab)
        if (tab == MainTab.Watch) {
            watchPage = WatchPage.Main
        } else {
            RouteTabs.pathFor(tab)?.let(webState::navigate)
        }
    }
    val watchNeedsAttention = WatchProblems.needsAttention(state)

    CompositionLocalProvider(LocalNowMs provides nowMs.longValue) {
        Column(
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .windowInsetsPadding(WindowInsets.statusBars)
                .imePadding(),
        ) {
            if (demo) DemoBanner()
            if (onLocal && remoteTarget != null && !nav.inWatch) {
                LocalCopyBanner(onTryServer = {
                    remoteFailed = false
                    forceRemote = true
                })
            }
            Row(Modifier.weight(1f).fillMaxWidth()) {
                if (navStyle == NavStyle.Rail) {
                    ShellRail(
                        selected = nav.highlighted,
                        watchNeedsAttention = watchNeedsAttention,
                        onTab = onTab,
                    )
                }
                Box(Modifier.weight(1f).fillMaxHeight()) {
                    if (webShown && target != null) {
                        ServerWebView(
                            target = target,
                            localWeb = if (onLocal) localWeb else null,
                            state = webState,
                            controller = controller,
                            sessionEpoch = account.sessionEpoch,
                            visible = webVisible,
                            onOpenWatch = {
                                nav = nav.openedWatch()
                                watchPage = WatchPage.Main
                            },
                            onRoute = { nav = nav.routed(it) },
                            modifier = Modifier.fillMaxSize(),
                        )
                    }
                    when {
                        nav.inWatch -> WatchTab(
                            state = state,
                            controller = controller,
                            page = watchPage,
                            onPage = { watchPage = it },
                            onSignIn = {
                                // From the account or server rows: leave the Watch tab for the sign-in screen.
                                forceRemote = true
                                nav = nav.tapped(nav.webTab)
                            },
                            // A native row that points at a web page: back to the web view, then the route.
                            onOpenWeb = { path ->
                                nav = nav.tapped(nav.webTab)
                                webState.navigate(path)
                            },
                        )
                        source == WebSource.Unlock -> UnlockScreen(
                            account = account,
                            onFingerprint = { activity -> controller.account.unlockWithFingerprint(activity) },
                            onPassword = { controller.account.unlockWithPassword(it) },
                            onOtherAccount = { controller.account.signOut() },
                            modifier = Modifier.background(MaterialTheme.colorScheme.background),
                        )
                        source == WebSource.Checking -> CheckingScreen(Modifier.background(MaterialTheme.colorScheme.background))
                        source == WebSource.SignIn || target == null -> SignInScreen(
                            account = account,
                            savedServer = rawServerUrl,
                            onSignIn = { server, username, password, code -> controller.account.signIn(server, username, password, code) },
                            modifier = Modifier.background(MaterialTheme.colorScheme.background),
                        )
                        inspection -> WebPreviewPlaceholder()
                        webState.unreachable -> ServerUnreachableScreen(
                            retrying = webState.retrying,
                            onRetry = webState::retry,
                            modifier = Modifier.background(MaterialTheme.colorScheme.background),
                        )
                    }
                }
            }
            if (navStyle == NavStyle.Bar) {
                ShellBar(
                    selected = nav.highlighted,
                    watchNeedsAttention = watchNeedsAttention,
                    onTab = onTab,
                )
            }
        }
    }
}

// ---------------------------------------------------------------- bottom bar

private data class BarItem(val tab: MainTab, @StringRes val label: Int, val icon: ImageVector)

private val BarItems = listOf(
    BarItem(MainTab.Today, R.string.tab_today, TodayIcon),
    BarItem(MainTab.Activities, R.string.tab_activities, ActivitiesIcon),
    BarItem(MainTab.Health, R.string.tab_health, HealthIcon),
    BarItem(MainTab.Watch, R.string.tab_watch, WatchIcon),
)

/** The canvas's bottom bar: raised background, a line on top, the selected tab in the link colour. */
@Composable
private fun ShellBar(selected: MainTab?, watchNeedsAttention: Boolean, onTab: (MainTab) -> Unit) {
    val line = NimragColors.Line
    NavigationBar(
        modifier = Modifier.drawBehind { drawLine(line, Offset(0f, 0f), Offset(size.width, 0f), strokeWidth = 1.dp.toPx()) },
        containerColor = NimragColors.Raised,
        contentColor = NimragColors.TextMuted,
        tonalElevation = 0.dp,
    ) {
        BarItems.forEach { item ->
            val label = stringResource(item.label)
            NavigationBarItem(
                selected = item.tab == selected,
                onClick = { onTab(item.tab) },
                icon = {
                    if (item.tab == MainTab.Watch && watchNeedsAttention) {
                        BadgedBox(badge = { Badge() }) { Icon(item.icon, contentDescription = null) }
                    } else {
                        Icon(item.icon, contentDescription = null)
                    }
                },
                label = { Text(label, style = MaterialTheme.typography.labelMedium) },
                colors = NavigationBarItemDefaults.colors(
                    selectedIconColor = NimragColors.Link,
                    selectedTextColor = NimragColors.Link,
                    unselectedIconColor = NimragColors.TextMuted,
                    unselectedTextColor = NimragColors.TextMuted,
                    indicatorColor = Color.Transparent,
                ),
            )
        }
    }
}

/** The same tabs as [ShellBar] for landscape: a narrow rail on the start edge, icons only (labels are the accessibility names). */
@Composable
private fun ShellRail(selected: MainTab?, watchNeedsAttention: Boolean, onTab: (MainTab) -> Unit) {
    val line = NimragColors.Line
    NavigationRail(
        modifier = Modifier.fillMaxHeight().drawBehind { drawLine(line, Offset(size.width, 0f), Offset(size.width, size.height), strokeWidth = 1.dp.toPx()) },
        containerColor = NimragColors.Raised,
        contentColor = NimragColors.TextMuted,
    ) {
        BarItems.forEach { item ->
            val label = stringResource(item.label)
            NavigationRailItem(
                selected = item.tab == selected,
                onClick = { onTab(item.tab) },
                icon = {
                    if (item.tab == MainTab.Watch && watchNeedsAttention) {
                        BadgedBox(badge = { Badge() }) { Icon(item.icon, contentDescription = label) }
                    } else {
                        Icon(item.icon, contentDescription = label)
                    }
                },
                alwaysShowLabel = false,
                colors = NavigationRailItemDefaults.colors(
                    selectedIconColor = NimragColors.Link,
                    unselectedIconColor = NimragColors.TextMuted,
                    indicatorColor = Color.Transparent,
                ),
            )
        }
    }
}

@Composable
private fun DemoBanner() {
    Text(
        stringResource(R.string.demo_banner),
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 6.dp),
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.tertiary,
    )
}
