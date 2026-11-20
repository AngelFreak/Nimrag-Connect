plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.library) apply false
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.kotlin.compose) apply false
}

// Licence guard. The `personal` flavor carries Garmin's FIT profile and must never be published.
// No release or publishing task of it (assemblePersonalRelease, bundlePersonalRelease, a bare
// `assembleRelease` that fans out to every flavor, signing, uploading) runs unless the caller
// says -Pnimrag.personal=true on purpose. Debug builds and unit tests of the personal flavor
// stay open: they are for this machine and need -Pnimrag.fitProfile=... anyway.
gradle.taskGraph.whenReady {
    val optedIn = providers.gradleProperty("nimrag.personal").orNull == "true"
    if (!optedIn) {
        val offending = allTasks.map { it.path }.filter(::isPersonalReleaseTask)
        if (offending.isNotEmpty()) {
            throw GradleException(
                "Refusing to build a release of the personal flavor (it contains Garmin's FIT profile " +
                    "and must never be published): ${offending.size} tasks, e.g. ${offending.take(3).joinToString()}. " +
                    "Build the public flavor (e.g. :app:assemblePublicRelease), or pass -Pnimrag.personal=true " +
                    "if this is the owner's own build on this machine.",
            )
        }
    }
}

fun isPersonalReleaseTask(path: String): Boolean {
    val name = path.substringAfterLast(':')
    if (!(path.startsWith(":app:") || path.startsWith(":core:"))) return false
    if (!name.contains("Personal")) return false
    // Any release variant task, and anything that publishes, uploads or signs (also of a debug
    // variant). Plain debug compile/package/test tasks are not matched.
    return Regex("Release|^(publish|upload|sign)|Publish|Upload").containsMatchIn(name)
}
