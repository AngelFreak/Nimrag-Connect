# The web view's JavaScript bridge (ui/shell/ServerWebView.kt): its @JavascriptInterface methods
# are called by name from the page, so R8 must keep them.
-keepclassmembers class dev.nimrag.app.ui.shell.NimragWebBridge {
    @android.webkit.JavascriptInterface <methods>;
}
-keepattributes JavascriptInterface
