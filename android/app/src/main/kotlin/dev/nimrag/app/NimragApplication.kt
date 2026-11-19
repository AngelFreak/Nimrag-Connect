package dev.nimrag.app

import android.app.Application
import dev.nimrag.app.engine.Hub
import dev.nimrag.app.engine.WatchEngine
import dev.nimrag.app.model.AppGraph

/** Creates the engine once per process and hands it to the UI through [AppGraph]. */
class NimragApplication : Application() {
    lateinit var engine: WatchEngine
        private set

    override fun onCreate() {
        super.onCreate()
        engine = WatchEngine(Hub(this))
        AppGraph.controller = engine
        engine.onProcessStart()
    }
}
