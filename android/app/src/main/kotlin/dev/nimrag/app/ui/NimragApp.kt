package dev.nimrag.app.ui

import androidx.activity.compose.BackHandler
import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import dev.nimrag.app.R
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.ui.components.LocalNowMs
import dev.nimrag.app.ui.components.WatchIcon
import dev.nimrag.app.ui.settings.LicencesScreen
import dev.nimrag.app.ui.settings.SettingsScreen
import dev.nimrag.app.ui.sync.SyncScreen
import dev.nimrag.app.ui.theme.NimragTheme
import dev.nimrag.app.ui.watch.WatchScreen
import kotlinx.coroutines.delay

private enum class Tab(@StringRes val label: Int, val icon: ImageVector) {
    Watch(R.string.tab_watch, WatchIcon),
    Sync(R.string.tab_sync, Icons.Filled.Refresh),
    Settings(R.string.tab_settings, Icons.Filled.Settings),
}

/** The whole app: theme, three tabs and the licences sub-screen. [demo] marks the fake controller. */
@Composable
fun NimragApp(controller: WatchController, demo: Boolean = false) {
    val state by controller.state.collectAsStateWithLifecycle()
    NimragTheme {
        NimragShell(state = state, controller = controller, demo = demo)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun NimragShell(
    state: AppState,
    controller: WatchController,
    demo: Boolean = false,
    initialTab: Int = 0,
) {
    var tabIndex by rememberSaveable { mutableIntStateOf(initialTab) }
    var showLicences by rememberSaveable { mutableStateOf(false) }
    val tab = Tab.entries[tabIndex]

    // Permissions and Bluetooth can change while we are away (system settings, quick tiles).
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { controller.refreshPermissions() }

    BackHandler(enabled = showLicences || tab != Tab.Watch) {
        if (showLicences) showLicences = false else tabIndex = Tab.Watch.ordinal
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

    val scrollBehavior = TopAppBarDefaults.enterAlwaysScrollBehavior()

    CompositionLocalProvider(LocalNowMs provides nowMs.longValue) {
        Scaffold(
            modifier = Modifier.nestedScroll(scrollBehavior.nestedScrollConnection),
            containerColor = MaterialTheme.colorScheme.background,
            topBar = {
                Column {
                    TopAppBar(
                        title = {
                            Text(
                                if (showLicences) stringResource(R.string.about_open_source) else stringResource(tab.label),
                            )
                        },
                        navigationIcon = {
                            if (showLicences) {
                                IconButton(onClick = { showLicences = false }) {
                                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                                }
                            }
                        },
                        colors = TopAppBarDefaults.topAppBarColors(
                            containerColor = MaterialTheme.colorScheme.background,
                            scrolledContainerColor = MaterialTheme.colorScheme.surfaceContainer,
                        ),
                        scrollBehavior = scrollBehavior,
                    )
                    if (demo) DemoBanner()
                }
            },
            bottomBar = {
                NavigationBar {
                    Tab.entries.forEach { entry ->
                        val label = stringResource(entry.label)
                        NavigationBarItem(
                            selected = entry == tab,
                            onClick = {
                                tabIndex = entry.ordinal
                                showLicences = false
                            },
                            icon = {
                                val attention = needsAttention(entry, state)
                                if (attention) {
                                    BadgedBox(badge = { Badge() }) { Icon(entry.icon, contentDescription = null) }
                                } else {
                                    Icon(entry.icon, contentDescription = null)
                                }
                            },
                            label = { Text(label) },
                        )
                    }
                }
            },
        ) { padding ->
            Box(Modifier.fillMaxSize()) {
                when {
                    showLicences -> LicencesScreen(contentPadding = padding)
                    tab == Tab.Watch -> WatchScreen(state, controller, padding)
                    tab == Tab.Sync -> SyncScreen(
                        state = state,
                        controller = controller,
                        contentPadding = padding,
                        onOpenSettings = { tabIndex = Tab.Settings.ordinal },
                    )
                    else -> SettingsScreen(
                        state = state,
                        controller = controller,
                        contentPadding = padding,
                        onOpenLicences = { showLicences = true },
                    )
                }
            }
        }
    }
}

/** A dot on a tab: something there needs the owner (permission missing, upload or sync trouble). */
private fun needsAttention(tab: Tab, state: AppState): Boolean = when (tab) {
    Tab.Watch -> !state.permissions.bluetooth || !state.permissions.bluetoothEnabled
    Tab.Sync -> state.uploads.lastError != null || (state.sync.lastResult?.failed ?: 0) > 0
    Tab.Settings -> false
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
