package dev.nimrag.app.ui.shell

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.res.painterResource
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.autofill.ContentType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentType
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.layout.ColumnScope
import dev.nimrag.app.R
import dev.nimrag.app.engine.auth.logic.AuthFailure
import dev.nimrag.app.engine.auth.logic.LoginForm
import dev.nimrag.app.model.AccountState
import dev.nimrag.app.ui.components.CardTitle
import dev.nimrag.app.ui.components.NimragButton
import dev.nimrag.app.ui.components.NimragOutlinedButton
import dev.nimrag.app.ui.components.SectionCard

/*
 * The two native account screens that come before the web app: sign-in (server, username,
 * password, code) and unlock (fingerprint or password). The password and the code live only in
 * `remember` (never saved instance state) and leave the screen with the call.
 */

/** Below this window height (dp) the logo shrinks, so the form gets the room (landscape). */
private const val COMPACT_HEIGHT_DP = 480

/**
 * The app's logo, as on the launcher: the icon's tile (background layer) with its foreground layer
 * (wordmark and sync arrow) on top. Both layers are the existing launcher vectors, drawn at the
 * zoom that fills a rounded tile (the adaptive layers keep a safe margin the tile does not need).
 */
@Composable
fun BrandLogo(size: Dp, modifier: Modifier = Modifier) {
    val description = stringResource(R.string.app_name)
    Box(
        modifier
            .size(size)
            .clip(RoundedCornerShape(size * 0.22f))
            .semantics { contentDescription = description },
    ) {
        Box(Modifier.fillMaxSize().graphicsLayer { scaleX = 1.5f; scaleY = 1.5f }) {
            Image(painterResource(R.drawable.ic_launcher_background), contentDescription = null, modifier = Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
            Image(painterResource(R.drawable.ic_launcher_foreground), contentDescription = null, modifier = Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
        }
    }
}

/**
 * The frame of every full-screen account step: scrolls when the content is taller than the window
 * (landscape, large text), keeps clear of the keyboard and the system bars, and centres a card of
 * at most 480 dp under the logo when there is room. Scrolling is the only way the whole form stays
 * reachable at 130% text in landscape, so nothing in here may use a fixed height.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun AccountPage(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    BoxWithConstraints(
        modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Horizontal))
            .imePadding(),
    ) {
        val compact = maxHeight < COMPACT_HEIGHT_DP.dp
        Column(
            Modifier
                .fillMaxWidth()
                .heightIn(min = maxHeight)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = if (compact) 12.dp else 20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(if (compact) 12.dp else 20.dp, Alignment.CenterVertically),
        ) {
            BrandLogo(if (compact) 56.dp else 88.dp)
            Column(Modifier.widthIn(max = 480.dp).fillMaxWidth(), content = content)
        }
    }
}

/** The one fixed sentence for a failure (never the server's own text). */
@Composable
fun authFailureText(failure: AuthFailure): String = when (failure) {
    AuthFailure.WrongCredentials -> stringResource(R.string.failure_wrong_credentials)
    AuthFailure.WrongPasswordOrRemoved -> stringResource(R.string.failure_wrong_password_or_removed)
    AuthFailure.SecretRevoked -> stringResource(R.string.failure_secret_revoked)
    AuthFailure.TokenRejected -> stringResource(R.string.failure_token_rejected)
    AuthFailure.AdminAccount -> stringResource(R.string.failure_admin)
    AuthFailure.TemporaryPassword -> stringResource(R.string.failure_temporary_password)
    AuthFailure.TokenCap -> stringResource(R.string.failure_token_cap)
    is AuthFailure.RateLimited -> {
        val seconds = failure.retryAfterSeconds
        when {
            seconds == null -> stringResource(R.string.failure_rate_limited_unknown)
            seconds <= 90 -> stringResource(R.string.failure_rate_limited_seconds, seconds)
            else -> stringResource(R.string.failure_rate_limited_minutes, (seconds + 59) / 60)
        }
    }
    AuthFailure.BadAddress -> stringResource(R.string.failure_bad_address)
    AuthFailure.Incomplete -> stringResource(R.string.failure_incomplete)
    AuthFailure.Network -> stringResource(R.string.failure_network)
    AuthFailure.Server -> stringResource(R.string.failure_server)
    AuthFailure.BiometricFailed -> stringResource(R.string.failure_biometric)
    AuthFailure.NoCredentials -> stringResource(R.string.failure_no_credentials)
}

@Composable
private fun FailureLine(failure: AuthFailure?) {
    if (failure == null) return
    Text(
        authFailureText(failure),
        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.error,
    )
}

@Composable
private fun Busy(text: String) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 3.dp)
        Text(text, style = MaterialTheme.typography.bodyMedium)
    }
}

/**
 * First sign-in. [savedServer] pre-fills the address (a phone signed out by the server keeps it).
 * [onSignIn] gets the raw fields; the account validates them and says what is wrong.
 */
@Composable
fun SignInScreen(
    account: AccountState,
    savedServer: String,
    onSignIn: (server: String, username: String, password: String, code: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    var server by rememberSaveable(savedServer) { mutableStateOf(savedServer) }
    var username by rememberSaveable { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    // A code is single-use: after any answer, start the field again.
    LaunchedEffect(account.failure) { if (account.failure != null) code = "" }
    val submit = { if (!account.busy) onSignIn(server, username, password, code) }

    AccountPage(modifier) {
        SectionCard {
            CardTitle(stringResource(R.string.signin_title))
            Text(
                stringResource(R.string.signin_body),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                value = server,
                onValueChange = { server = it },
                modifier = Modifier.fillMaxWidth(),
                label = { Text(stringResource(R.string.signin_server)) },
                placeholder = { Text(stringResource(R.string.server_url_placeholder)) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false, imeAction = ImeAction.Next),
            )
            OutlinedTextField(
                value = username,
                onValueChange = { username = it },
                modifier = Modifier.fillMaxWidth().semantics { contentType = ContentType.Username },
                label = { Text(stringResource(R.string.signin_username)) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Text, autoCorrectEnabled = false, imeAction = ImeAction.Next),
            )
            OutlinedTextField(
                value = password,
                onValueChange = { password = it },
                modifier = Modifier.fillMaxWidth().semantics { contentType = ContentType.Password },
                label = { Text(stringResource(R.string.signin_password)) },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false, imeAction = ImeAction.Next),
            )
            OutlinedTextField(
                value = code,
                onValueChange = { code = LoginForm.filterCode(it) },
                modifier = Modifier.fillMaxWidth().semantics { contentType = ContentType.SmsOtpCode },
                label = { Text(stringResource(R.string.signin_code)) },
                supportingText = { Text(stringResource(R.string.signin_code_hint)) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword, autoCorrectEnabled = false, imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { submit() }),
            )
            FailureLine(account.failure)
            if (account.busy) {
                Busy(stringResource(R.string.signin_busy))
            } else {
                NimragButton(onClick = { submit() }, modifier = Modifier.fillMaxWidth()) { Text(stringResource(R.string.signin_button)) }
            }
        }
    }
}

/**
 * The session ended on a phone this account trusts: fingerprint (when set up) or password, no code.
 * [onFingerprint] runs the system prompt; [onOtherAccount] signs this phone out.
 */
@Composable
fun UnlockScreen(
    account: AccountState,
    onFingerprint: (Activity) -> Unit,
    onPassword: (String) -> Unit,
    onOtherAccount: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var password by remember { mutableStateOf("") }
    LaunchedEffect(account.failure) { if (account.failure != null) password = "" }
    val activity = LocalContext.current.findActivity()
    val inspection = LocalInspectionMode.current
    // Offer the fingerprint straight away, once per time the screen opens (not again after a cancel).
    var prompted by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(account.biometricReady, activity) {
        if (account.biometricReady && !prompted && activity != null && !inspection) {
            prompted = true
            onFingerprint(activity)
        }
    }
    val submit = { if (!account.busy) onPassword(password) }

    AccountPage(modifier) {
        SectionCard {
            CardTitle(stringResource(R.string.unlock_title))
            Text(
                stringResource(R.string.unlock_body, account.username.orEmpty()),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (account.biometricReady && activity != null) {
                NimragButton(onClick = { if (!account.busy) onFingerprint(activity) }, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.unlock_fingerprint))
                }
            } else {
                Text(
                    stringResource(R.string.unlock_no_fingerprint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            OutlinedTextField(
                value = password,
                onValueChange = { password = it },
                modifier = Modifier.fillMaxWidth().semantics { contentType = ContentType.Password },
                label = { Text(stringResource(R.string.unlock_password)) },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false, imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { submit() }),
            )
            FailureLine(account.failure)
            if (account.busy) {
                Busy(stringResource(R.string.unlock_busy))
            } else {
                NimragOutlinedButton(onClick = { submit() }, modifier = Modifier.fillMaxWidth()) { Text(stringResource(R.string.unlock_button)) }
            }
            TextButton(onClick = onOtherAccount, modifier = Modifier.fillMaxWidth()) { Text(stringResource(R.string.unlock_other_account)) }
        }
    }
}

/** Shown for the moment the first check of the session takes (the web view is not created yet). */
@Composable
fun CheckingScreen(modifier: Modifier = Modifier) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
            CircularProgressIndicator(strokeWidth = 3.dp)
            Text(
                stringResource(R.string.checking_session),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

internal tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
