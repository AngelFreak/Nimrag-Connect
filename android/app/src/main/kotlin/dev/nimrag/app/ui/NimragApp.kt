package dev.nimrag.app.ui

import androidx.activity.compose.BackHandler
import androidx.annotation.StringRes
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
import dev.nimrag.app.ui.settings.LicencesScreen
import dev.nimrag.app.ui.settings.SettingsScreen
import dev.nimrag.app.ui.shell.ServerNeededScreen
import dev.nimrag.app.ui.shell.ServerUnreachableScreen
import dev.nimrag.app.ui.shell.ServerWebView
import dev.nimrag.app.ui.shell.WebPreviewPlaceholder
import dev.nimrag.app.ui.shell.logic.BridgeStatus
import dev.nimrag.app.ui.shell.logic.MainTab
import dev.nimrag.app.ui.shell.logic.RouteTabs
import dev.nimrag.app.ui.shell.logic.ServerOrigin
import dev.nimrag.app.ui.shell.logic.ShellNav
import dev.nimrag.app.ui.shell.logic.WebSource
import dev.nimrag.app.ui.shell.logic.WebSourcePolicy
import dev.nimrag.app.ui.shell.LocalCopyBanner
import dev.nimrag.app.ui.shell.isOnline
import androidx.compose.ui.platform.LocalContext
import dev.nimrag.app.ui.shell.rememberWebShellState
import dev.nimrag.app.ui.sync.SyncScreen
import dev.nimrag.app.ui.theme.NimragColors
import dev.nimrag.app.ui.theme.NimragTheme
import dev.nimrag.app.ui.watch.WatchScreen
import kotlinx.coroutines.delay

/** The Watch tab's own pages (the existing Watch, Sync and Settings screens). */
internal enum class WatchSection(@StringRes val label: Int, val icon: ImageVector) {
    Watch(R.string.tab_watch, WatchIcon),
    Sync(R.string.tab_sync, Icons.Filled.Refresh),
    Settings(R.string.tab_settings, Icons.Filled.Settings),
}

private val NavSaver = listSaver<ShellNav, Any>(
    save = { listOf(it.webTab.ordinal, it.inWatch) },
    restore = { ShellNav(MainTab.entries[it[0] as Int], it[1] as Boolean) },
)

/**
 * The whole app: theme, the native bottom bar (Today, Activities, Health, Watch), one
 * shared web view for the first four and the native Watch, Sync and Settings screens under Watch.
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
    initialSection: Int = 0,
    startInWatch: Boolean = false,
) {
    var nav by rememberSaveable(stateSaver = NavSaver) { mutableStateOf(ShellNav(inWatch = startInWatch)) }
    var sectionIndex by rememberSaveable { mutableIntStateOf(initialSection) }
    var showLicences by rememberSaveable { mutableStateOf(false) }
    val section = WatchSection.entries[sectionIndex]

    val rawServerUrl = state.settings.serverUrl
    val remoteTarget = remember(rawServerUrl) { ServerOrigin.parse(rawServerUrl) }
    val webState = rememberWebShellState()
    val inspection = LocalInspectionMode.current
    val context = LocalContext.current

    // The phone's own copy of the data (personal flavor): used when the server is not the better
    // choice (see WebSourcePolicy). Decided when the web view is created, never while a page is open.
    val localWeb = remember(state.localWebPort) { if (state.localWebPort != null) controller.localWeb() else null }
    var remoteFailed by remember { mutableStateOf(false) }
    var forceRemote by remember { mutableStateOf(false) }
    val source = remember(localWeb, rawServerUrl, remoteFailed, forceRemote) {
        if (forceRemote) {
            WebSource.Remote
        } else {
            WebSourcePolicy.choose(
                mode = controller.webMode(),
                localReady = localWeb != null,
                remoteConfigured = remoteTarget != null,
                online = isOnline(context),
                remoteFailed = remoteFailed,
                pendingUploads = state.uploads.pending,
            )
        }
    }
    val onLocal = source == WebSource.Local && localWeb != null
    val target = if (onLocal) ServerOrigin.parse(localWeb.startUrl) else remoteTarget
    // The server did not answer and the phone has a copy: switch to it for the rest of this run.
    LaunchedEffect(webState.unreachable, localWeb) {
        if (webState.unreachable && localWeb != null && !onLocal && !forceRemote) remoteFailed = true
    }

    // Permissions and Bluetooth can change while we are away (system settings, quick tiles).
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        controller.refreshPermissions()
        webState.onResume()
    }
    LifecycleEventEffect(Lifecycle.Event.ON_PAUSE) { webState.onPause() }

    // The page learns about link and last-sync changes (and only changes).
    val bridgeStatus = BridgeStatus.of(state)
    LaunchedEffect(bridgeStatus) { webState.dispatchStatus(bridgeStatus) }

    // Back: web history first; in the Watch tab its sub-pages first, then the web tabs; then exit.
    val backEnabled = if (nav.inWatch) showLicences || sectionIndex != 0 || target != null else webState.canGoBack
    BackHandler(enabled = backEnabled) {
        when {
            !nav.inWatch -> webState.goBack()
            showLicences -> showLicences = false
            sectionIndex != 0 -> sectionIndex = 0
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

    val openSettings = {
        nav = nav.openedWatch()
        sectionIndex = WatchSection.Settings.ordinal
        showLicences = false
    }
    val imeVisible = WindowInsets.isImeVisible

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
            Box(Modifier.weight(1f).fillMaxWidth()) {
                if (target != null && !inspection) {
                    ServerWebView(
                        target = target,
                        localWeb = if (onLocal) localWeb else null,
                        state = webState,
                        controller = controller,
                        visible = !nav.inWatch,
                        onOpenWatch = {
                            nav = nav.openedWatch()
                            sectionIndex = 0
                            showLicences = false
                        },
                        onRoute = { nav = nav.routed(it) },
                        modifier = Modifier.fillMaxSize(),
                    )
                }
                when {
                    nav.inWatch -> WatchArea(
                        state = state,
                        controller = controller,
                        section = section,
                        onSection = {
                            sectionIndex = it.ordinal
                            showLicences = false
                        },
                        showLicences = showLicences,
                        onShowLicences = { showLicences = it },
                    )
                    target == null -> ServerNeededScreen(
                        invalidAddress = rawServerUrl.takeIf { ServerOrigin.isInvalid(it) },
                        onOpenSettings = openSettings,
                    )
                    inspection -> WebPreviewPlaceholder()
                    webState.unreachable -> ServerUnreachableScreen(
                        retrying = webState.retrying,
                        onRetry = webState::retry,
                        modifier = Modifier.background(MaterialTheme.colorScheme.background),
                    )
                }
            }
            if (!imeVisible) {
                ShellBar(
                    selected = nav.highlighted,
                    watchNeedsAttention = WatchSection.entries.any { needsAttention(it, state) },
                    onTab = { tab ->
                        nav = nav.tapped(tab)
                        if (tab == MainTab.Watch) {
                            showLicences = false
                        } else {
                            RouteTabs.pathFor(tab)?.let(webState::navigate)
                        }
                    },
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
private fun ShellBar(selected: MainTab, watchNeedsAttention: Boolean, onTab: (MainTab) -> Unit) {
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

// ---------------------------------------------------------------- Watch tab

/** "Devices and sync": header, a switch between the three native pages, and the page itself. */
@Composable
private fun WatchArea(
    state: AppState,
    controller: WatchController,
    section: WatchSection,
    onSection: (WatchSection) -> Unit,
    showLicences: Boolean,
    onShowLicences: (Boolean) -> Unit,
) {
    val noPadding = PaddingValues(0.dp)
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        DevicesHeader(showLicences = showLicences, onBack = { onShowLicences(false) })
        if (!showLicences) {
            SectionSwitch(section, state, onSection)
        }
        Box(Modifier.weight(1f).fillMaxWidth()) {
            when {
                showLicences -> LicencesScreen(contentPadding = noPadding)
                section == WatchSection.Watch -> WatchScreen(state, controller, noPadding)
                section == WatchSection.Sync -> SyncScreen(
                    state = state,
                    controller = controller,
                    contentPadding = noPadding,
                    onOpenSettings = { onSection(WatchSection.Settings) },
                )
                else -> SettingsScreen(
                    state = state,
                    controller = controller,
                    contentPadding = noPadding,
                    onOpenLicences = { onShowLicences(true) },
                )
            }
        }
    }
}

@Composable
private fun DevicesHeader(showLicences: Boolean, onBack: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(start = if (showLicences) 12.dp else 20.dp, end = 20.dp, top = 12.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (showLicences) {
            IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
            }
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                if (showLicences) stringResource(R.string.about_open_source) else stringResource(R.string.devices_title),
                style = MaterialTheme.typography.titleLarge,
                color = MaterialTheme.colorScheme.onBackground,
            )
            if (!showLicences) {
                Text(
                    stringResource(R.string.devices_subtitle),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

/** Three equal pills (Watch, Sync, Settings) on a card; the open one in the link colour. */
@Composable
private fun SectionSwitch(selected: WatchSection, state: AppState, onSelect: (WatchSection) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 20.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerLow)
            .border(BorderStroke(1.dp, NimragColors.Line), RoundedCornerShape(12.dp))
            .padding(4.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        WatchSection.entries.forEach { entry ->
            val isSelected = entry == selected
            val label = stringResource(entry.label)
            val attention = needsAttention(entry, state)
            Box(
                Modifier
                    .weight(1f)
                    .heightIn(min = 44.dp)
                    .clip(RoundedCornerShape(9.dp))
                    .background(if (isSelected) MaterialTheme.colorScheme.primaryContainer else Color.Transparent)
                    .clickable(role = Role.Tab, onClick = { onSelect(entry) })
                    .semantics { contentDescription = label },
                contentAlignment = Alignment.Center,
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(
                        label,
                        style = MaterialTheme.typography.labelLarge,
                        color = if (isSelected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (attention) Badge()
                }
            }
        }
    }
}

/** A dot on a page: something there needs the owner (permission missing, upload or sync trouble). */
private fun needsAttention(section: WatchSection, state: AppState): Boolean = when (section) {
    WatchSection.Watch -> !state.permissions.bluetooth || !state.permissions.bluetoothEnabled
    WatchSection.Sync -> state.uploads.lastError != null || (state.sync.lastResult?.failed ?: 0) > 0
    WatchSection.Settings -> false
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
