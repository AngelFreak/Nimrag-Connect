package dev.nimrag.app

import android.app.Activity
import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.result.IntentSenderRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.setContent
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import dev.nimrag.app.model.AppGraph
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.ui.NimragApp
import dev.nimrag.app.ui.preview.FakeController
import kotlinx.coroutines.launch
import uniffi.nimrag_ffi.fitProfileCompiledIn

/** The single activity: hosts the Compose UI, which reads [AppGraph.controller] only. */
class MainActivity : ComponentActivity() {
    private val engine get() = (application as? NimragApplication)?.engine

    /** The system's companion-device confirmation dialog (pairing step 1, see CompanionAssociation). */
    private val associationLauncher = registerForActivityResult(ActivityResultContracts.StartIntentSenderForResult()) {
        engine?.onAssociationResult(it.resultCode == Activity.RESULT_OK)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Licence rule: the FIT profile (Garmin data) must never be in an Android build.
        check(!fitProfileCompiledIn()) { "FIT profile compiled into the Android library" }
        // Dark only: light system bar icons on transparent bars, like the web interface.
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )

        // The Application class (engine) sets AppGraph.controller. Without it (a UI-only build)
        // the shell still opens, on a scripted demo controller.
        val real: WatchController? = try {
            AppGraph.controller
        } catch (_: UninitializedPropertyAccessException) {
            null
        }
        val controller = real ?: FakeController.demo
        engine?.let { e ->
            lifecycleScope.launch {
                repeatOnLifecycle(Lifecycle.State.STARTED) {
                    e.associationChooser.collect { sender ->
                        if (sender != null) {
                            e.onAssociationLaunched()
                            associationLauncher.launch(IntentSenderRequest.Builder(sender).build())
                        }
                    }
                }
            }
        }
        setContent { NimragApp(controller = controller, demo = real == null) }
    }
}
