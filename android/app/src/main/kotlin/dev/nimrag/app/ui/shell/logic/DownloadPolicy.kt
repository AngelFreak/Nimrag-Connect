package dev.nimrag.app.ui.shell.logic

/*
 * Pure rules for downloads the web view starts (IMPLEMENTATION_PLAN.md, stage 4c: "Download my
 * data" in the web Profile is a plain link to /api/export). A web view saves nothing by itself;
 * the shell hands a download to Android's DownloadManager, with the session cookie. No Android
 * types here, so the unit tests run on the JVM.
 */
object DownloadPolicy {
    const val FALLBACK_NAME = "nimrag-download"

    /**
     * Only a download from the server's own origin is taken (it carries the session cookie, which
     * must never go anywhere else). The web view does not load other origins at all, so this is a
     * second line.
     */
    fun allow(url: String?, server: Origin?): Boolean = ServerOrigin.isSameOrigin(url, server)

    /**
     * The file name to save as: the `filename` of Content-Disposition (quoted or not), else the
     * last path segment of [url], else [FALLBACK_NAME]. Only letters, digits, dot, dash and
     * underscore survive, and no leading dot, so the name cannot climb out of the Downloads folder.
     */
    fun fileName(contentDisposition: String?, url: String?): String {
        val fromHeader = contentDisposition?.let { FILENAME.find(it)?.groupValues?.get(1) }
        val fromUrl = url?.substringBefore('?')?.substringBefore('#')?.trimEnd('/')?.substringAfterLast('/')
        for (raw in listOf(fromHeader, fromUrl)) {
            val clean = raw.orEmpty()
                .substringAfterLast('/')
                .substringAfterLast('\\')
                .map { if (it.isLetterOrDigit() && it.code < 128 || it == '.' || it == '-' || it == '_') it else '_' }
                .joinToString("")
                .trimStart('.')
                .take(100)
            if (clean.isNotEmpty() && clean.any { it.isLetterOrDigit() }) return clean
        }
        return FALLBACK_NAME
    }

    private val FILENAME = Regex("""filename\s*=\s*"?([^";]+)"?""", RegexOption.IGNORE_CASE)
}
