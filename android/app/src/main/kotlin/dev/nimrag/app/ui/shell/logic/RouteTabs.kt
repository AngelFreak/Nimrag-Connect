package dev.nimrag.app.ui.shell.logic

/** The four destinations of the native bottom bar. The first three are the web view. */
enum class MainTab(val webPath: String?) {
    Today("/"),
    Activities("/activities"),
    Health("/health"),
    Watch(null),
    ;

    val isWeb: Boolean get() = webPath != null
}

/**
 * Which tab a web route belongs to. Mirrors `navKeyFor` in web/src/components/AppShell.tsx so the
 * native bar and the web app agree; routes without a tab (profile, Send to Garmin, devices,
 * unknown) give null and the bar highlights nothing, as the web app does.
 */
object RouteTabs {
    val webTabs: List<MainTab> = MainTab.entries.filter { it.isWeb }

    /** The route a tab tap sends to the web app. */
    fun pathFor(tab: MainTab): String? = tab.webPath

    fun tabForRoute(route: String?): MainTab? {
        if (route.isNullOrBlank()) return null
        val noFragment = route.substringBefore('#')
        val path = noFragment.substringBefore('?').trimEnd('/').ifEmpty { "/" }
        // Strength is a filter of Activities (no tab of its own).
        return when {
            path == "/" || under(path, "/day") || under(path, "/calendar") -> MainTab.Today
            path == "/strength" || under(path, "/activities") || under(path, "/workouts") -> MainTab.Activities
            under(path, "/sleep") || under(path, "/health") || under(path, "/movement") ||
                under(path, "/training") || under(path, "/body") -> MainTab.Health
            else -> null
        }
    }

    /** [path] is [prefix] itself or below it (`/day` and `/day/2026-11-19`, not `/daylight`). */
    private fun under(path: String, prefix: String): Boolean =
        path == prefix || path.startsWith("$prefix/")
}

/**
 * What the bar highlights and where the shell is. [webTab] is the last tab the web app reported
 * (or the owner tapped); [inWatch] is whether the native Watch tab is on screen; [tabless] is
 * whether the page shows a route without a tab (then nothing is highlighted, but [webTab] stays
 * as the tab the web view belongs to). The web view stays alive behind the Watch tab, so a route
 * the page reports meanwhile only changes [webTab] and [tabless].
 */
data class ShellNav(val webTab: MainTab = MainTab.Today, val inWatch: Boolean = false, val tabless: Boolean = false) {
    val highlighted: MainTab?
        get() = when {
            inWatch -> MainTab.Watch
            tabless -> null
            else -> webTab
        }

    /** The owner tapped [tab] in the bar. */
    fun tapped(tab: MainTab): ShellNav = if (tab == MainTab.Watch) copy(inWatch = true) else ShellNav(tab, false)

    /** `NimragApp.openWatch()` from the page. */
    fun openedWatch(): ShellNav = copy(inWatch = true)

    /** `NimragApp.onRoute(path)` from the page; a missing route changes nothing. */
    fun routed(route: String?): ShellNav {
        if (route.isNullOrBlank()) return this
        return RouteTabs.tabForRoute(route)?.let { copy(webTab = it, tabless = false) } ?: copy(tabless = true)
    }
}
