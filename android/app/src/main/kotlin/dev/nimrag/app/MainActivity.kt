package dev.nimrag.app

import android.app.Activity
import android.os.Bundle
import android.widget.TextView
import uniffi.nimrag_ffi.ffiVersion
import uniffi.nimrag_ffi.fitProfileCompiledIn

/** Placeholder shell: shows the core version string from the Rust library. Stage 2c fills it. */
class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Licence rule: the FIT profile (Garmin data) must never be in an Android build.
        check(!fitProfileCompiledIn()) { "FIT profile compiled into the Android library" }
        val text = TextView(this)
        text.text = "Nimrag Connect\ncore ${ffiVersion()}"
        text.textSize = 20f
        text.setPadding(48, 96, 48, 48)
        setContentView(text)
    }
}
