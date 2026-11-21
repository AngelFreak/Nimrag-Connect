package dev.nimrag.app.engine.phone

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.nimrag.app.NimragApplication
import dev.nimrag.app.R
import dev.nimrag.app.model.AppGraph
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.ui.components.NimragButton
import dev.nimrag.app.ui.theme.NimragTheme

/** "Found it" on the notification: stops the ringing. Only our own PendingIntent sends it. */
class FindPhoneReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != ACTION_STOP) return
        (context.applicationContext as? NimragApplication)?.engine?.stopFindPhone()
    }

    companion object {
        const val ACTION_STOP = "dev.nimrag.app.action.FIND_PHONE_STOP"
    }
}

/**
 * The full-screen "Found it" page shown over the lock screen while the phone rings. It closes
 * itself when the ringing stops, whoever stopped it.
 */
class FindPhoneActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setShowWhenLocked(true)
        setTurnScreenOn(true)
        val controller: WatchController? = try {
            AppGraph.controller
        } catch (_: UninitializedPropertyAccessException) {
            null
        }
        if (controller == null) {
            finish()
            return
        }
        setContent {
            NimragTheme {
                FindPhoneScreen(controller, onDone = { finish() })
            }
        }
    }
}

@Composable
private fun FindPhoneScreen(controller: WatchController, onDone: () -> Unit) {
    val state by controller.state.collectAsStateWithLifecycle()
    val ringing = state.phone.findPhoneRinging
    // The ringing started before this page opened; once it is over, so is the page.
    LaunchedEffect(ringing) { if (!ringing) onDone() }
    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Column(
            Modifier.fillMaxSize().padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(24.dp, Alignment.CenterVertically),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                stringResource(R.string.find_phone_title),
                style = MaterialTheme.typography.headlineMedium,
                textAlign = TextAlign.Center,
            )
            Text(
                stringResource(R.string.find_phone_text),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
            NimragButton(onClick = controller::stopFindPhone, modifier = Modifier.fillMaxWidth().heightIn(min = 72.dp)) {
                Text(stringResource(R.string.find_phone_found), style = MaterialTheme.typography.titleLarge)
            }
        }
    }
}
