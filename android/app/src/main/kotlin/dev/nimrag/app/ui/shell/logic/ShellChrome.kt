package dev.nimrag.app.ui.shell.logic

/** How the shell offers its four tabs. */
enum class NavStyle {
    /** No navigation: a full-screen account step (sign-in, server address, code, unlock) or the first check. */
    None,

    /** The bottom bar with icons and labels (portrait, tall windows). */
    Bar,

    /** A narrow rail on the start edge, icons only (landscape, short windows). */
    Rail,
}

/**
 * Pure rules for the shell's own chrome (no Android types): when the tab bar is shown at all, and
 * whether it sits at the bottom or on the side.
 */
object ShellChrome {
    /** Windows shorter than this (dp) get the compact rail instead of the bottom bar. */
    const val COMPACT_HEIGHT_DP = 480

    /**
     * True while the web area is an account step instead of the app: the native sign-in or unlock
     * screen, the first session check, or no server address yet (which also shows sign-in).
     */
    fun accountStepShown(source: WebSource, hasTarget: Boolean): Boolean =
        !hasTarget || source == WebSource.SignIn || source == WebSource.Unlock || source == WebSource.Checking

    /** Short windows (landscape phones) cannot spare a third of their height for a bar. */
    fun useRail(windowHeightDp: Int): Boolean = windowHeightDp < COMPACT_HEIGHT_DP

    /**
     * The navigation to draw. The Watch tab's own screens always keep it (it is the way back out);
     * an account step hides it until the person is signed in. The bottom bar also gives way to the
     * keyboard; the rail is narrow and stays.
     */
    fun navStyle(
        source: WebSource,
        hasTarget: Boolean,
        inWatch: Boolean,
        windowHeightDp: Int,
        imeVisible: Boolean,
    ): NavStyle = when {
        !inWatch && accountStepShown(source, hasTarget) -> NavStyle.None
        useRail(windowHeightDp) -> NavStyle.Rail
        imeVisible -> NavStyle.None
        else -> NavStyle.Bar
    }
}
