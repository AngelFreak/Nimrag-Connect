package dev.nimrag.app.ui.watch.logic

/**
 * The pages of the Watch tab: the page itself and the sub-pages behind its rows. Back goes to
 * [parent]; the page the person is on is kept across rotation by the shell.
 */
enum class WatchPage(val parent: WatchPage?) {
    Main(null),
    AutoSync(Main),
    DataRetention(Main),
    GarminConnect(Main),
    GarminHelp(Main),
    WatchDetails(Main),
    SyncHistory(Main),
    PhoneFeatures(Main),
    NotificationApps(PhoneFeatures),
    Server(Main),
    Account(Main),
    About(Main),
    Advanced(About),
    Licences(About),
    Help(Main),
}
