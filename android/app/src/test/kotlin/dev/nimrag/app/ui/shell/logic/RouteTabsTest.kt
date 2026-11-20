package dev.nimrag.app.ui.shell.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class RouteTabsTest {
    @Test
    fun `the three web tabs send their paths`() {
        assertEquals("/", RouteTabs.pathFor(MainTab.Today))
        assertEquals("/activities", RouteTabs.pathFor(MainTab.Activities))
        assertEquals("/health", RouteTabs.pathFor(MainTab.Health))
        assertNull(RouteTabs.pathFor(MainTab.Watch))
        assertEquals(listOf(MainTab.Today, MainTab.Activities, MainTab.Health), RouteTabs.webTabs)
        assertFalse(MainTab.Watch.isWeb)
        assertTrue(MainTab.Today.isWeb)
    }

    @Test
    fun `top level routes map to their tab`() {
        assertEquals(MainTab.Today, RouteTabs.tabForRoute("/"))
        assertEquals(MainTab.Activities, RouteTabs.tabForRoute("/activities"))
        assertEquals(MainTab.Activities, RouteTabs.tabForRoute("/activities/"))
        assertEquals(MainTab.Health, RouteTabs.tabForRoute("/health"))
        // Strength is an Activities filter, not a tab.
        assertEquals(MainTab.Activities, RouteTabs.tabForRoute("/strength"))
    }

    @Test
    fun `detail routes follow the web app's own nav key`() {
        assertEquals(MainTab.Activities, RouteTabs.tabForRoute("/activities/abc123"))
        assertEquals(MainTab.Activities, RouteTabs.tabForRoute("/activities/abc123/strength"))
        assertEquals(MainTab.Activities, RouteTabs.tabForRoute("/activities?type=strength"))
        assertEquals(MainTab.Activities, RouteTabs.tabForRoute("/activities?before=5&type=strength"))
        assertEquals(MainTab.Activities, RouteTabs.tabForRoute("/activities?type=run"))
        assertEquals(MainTab.Activities, RouteTabs.tabForRoute("/workouts/new"))
        assertEquals(MainTab.Today, RouteTabs.tabForRoute("/day/2026-11-19"))
        assertEquals(MainTab.Today, RouteTabs.tabForRoute("/day"))
        assertEquals(MainTab.Today, RouteTabs.tabForRoute("/calendar/2026-11"))
        for (p in listOf("/sleep", "/sleep/2026-11-19", "/movement", "/training", "/body")) {
            assertEquals(MainTab.Health, RouteTabs.tabForRoute(p), p)
        }
    }

    @Test
    fun `query and fragment do not change the path match`() {
        assertEquals(MainTab.Today, RouteTabs.tabForRoute("/?x=1"))
        assertEquals(MainTab.Health, RouteTabs.tabForRoute("/health#top"))
        assertEquals(MainTab.Activities, RouteTabs.tabForRoute("/activities?type=strength#list"))
    }

    @Test
    fun `routes without a tab and lookalike prefixes give null`() {
        for (p in listOf(null, "", "  ", "/login", "/login?next=%2F", "/settings", "/devices", "/nope", "/daylight", "/activitiesX", "/healthy", "__gallery")) {
            assertNull(RouteTabs.tabForRoute(p), "'$p'")
        }
    }

    @Test
    fun `a tab tap highlights that tab and leaves the watch`() {
        val nav = ShellNav().tapped(MainTab.Health)
        assertEquals(MainTab.Health, nav.highlighted)
        assertFalse(nav.inWatch)
        assertEquals(MainTab.Watch, nav.tapped(MainTab.Watch).highlighted)
        assertEquals(MainTab.Activities, nav.tapped(MainTab.Watch).tapped(MainTab.Activities).highlighted)
    }

    @Test
    fun `opening the watch keeps the web tab to come back to`() {
        val nav = ShellNav(MainTab.Activities).openedWatch()
        assertEquals(MainTab.Watch, nav.highlighted)
        assertEquals(MainTab.Activities, nav.webTab)
    }

    @Test
    fun `a reported route moves the highlight on a web tab`() {
        val nav = ShellNav(MainTab.Today).routed("/activities/9")
        assertEquals(MainTab.Activities, nav.highlighted)
    }

    @Test
    fun `a route reported while the watch is shown does not leave the watch`() {
        val nav = ShellNav(MainTab.Today, inWatch = true).routed("/health")
        assertEquals(MainTab.Watch, nav.highlighted)
        assertEquals(MainTab.Health, nav.webTab)
    }

    @Test
    fun `an unknown route keeps the highlight`() {
        val nav = ShellNav(MainTab.Health)
        assertEquals(nav, nav.routed("/login"))
        assertEquals(nav, nav.routed(null))
    }
}
