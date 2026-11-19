# UniFFI loads the native library through JNA, which reflects on these.
-keep class com.sun.jna.** { *; }
-keep class uniffi.nimrag_ffi.** { *; }
-dontwarn java.awt.*
