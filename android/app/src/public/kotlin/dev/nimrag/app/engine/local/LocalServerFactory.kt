package dev.nimrag.app.engine.local

import android.app.Application
import dev.nimrag.app.engine.store.EngineDb
import dev.nimrag.app.engine.store.EngineLog
import dev.nimrag.app.engine.store.FileStore

/** `public` flavor: no local server (the library has none). The web view always shows the server. */
object LocalServerFactory {
    @Suppress("UNUSED_PARAMETER")
    fun create(
        app: Application,
        db: EngineDb,
        store: FileStore,
        log: EngineLog,
        onData: () -> Unit,
        onRunning: (Int?) -> Unit,
    ): LocalServerHost = object : LocalServerHost {
        override fun start() = Unit
        override fun onFileStored() = Unit
        override val web: LocalWeb? get() = null
    }
}
