package dev.nimrag.app.engine.phone

import dev.nimrag.app.engine.Hub
import dev.nimrag.app.engine.ble.WatchLink
import dev.nimrag.app.engine.phone.logic.ChangeFilter
import dev.nimrag.app.engine.phone.logic.Decision
import dev.nimrag.app.engine.phone.logic.DutyMask
import dev.nimrag.app.engine.phone.logic.FindPhoneRules
import dev.nimrag.app.engine.phone.logic.NotificationIds
import dev.nimrag.app.engine.phone.logic.NotificationMapper
import dev.nimrag.app.engine.phone.logic.NotificationPolicy
import dev.nimrag.app.engine.phone.logic.NotificationSnapshot
import dev.nimrag.app.engine.phone.logic.WatchCategory
import dev.nimrag.app.engine.phone.logic.WatchMusicCommand
import dev.nimrag.app.model.FindWatchUi
import uniffi.nimrag_ffi.Event
import uniffi.nimrag_ffi.FindWatchState
import uniffi.nimrag_ffi.Input
import uniffi.nimrag_ffi.MusicCommand
import uniffi.nimrag_ffi.NotificationAction
import uniffi.nimrag_ffi.NotificationCategory
import uniffi.nimrag_ffi.PhoneNotification

/**
 * What the phone does for the watch besides syncing (stage 2f): notifications on the watch, the
 * watch ringing the phone, music control and the watch ringing on request. It sits between the
 * Android side (the notification listener, the media sessions, the alarm) and the protocol
 * client in [WatchLink].
 *
 * Threading: everything runs on the engine's HandlerThread ([Hub.handler]); the listener service
 * posts onto it. The one exception is [forwarding], a flag the listener reads on its own thread
 * so that a switched-off feature never even looks at a notification.
 *
 * Privacy: notification, music and reply content only passes through here in memory. Nothing
 * here writes it to the log, to settings or to the database; log lines state what happened,
 * never to what or from whom.
 */
class PhoneBridge(private val hub: Hub) {
    private var link: WatchLink? = null
    private val ids = NotificationIds()
    private val changes = ChangeFilter()
    private val alarm = FindPhoneAlarm(hub.app) { ringing ->
        hub.update { it.copy(phone = it.phone.copy(findPhoneRinging = ringing)) }
    }
    private val music = MusicBridge(hub) { input -> link?.sendDuty(input) }
    private val ownPackage = hub.app.packageName

    /** The watch has subscribed to notifications on this connection. */
    private var subscribed = false

    /** True while notifications are wanted: the duty is on for this connection and the watch subscribed. */
    @Volatile
    var forwarding = false
        private set

    private val resetFindWatch = Runnable { setFindWatch(FindWatchUi.Idle) }

    fun attach(link: WatchLink) {
        this.link = link
    }

    // ---- link life cycle (from WatchLink, on the handler) ----

    /** The handshake is done on a connection built with [duties]. */
    fun onLinkReady(duties: DutyMask) {
        resetNotifications()
        if (duties.music) music.start()
        refreshForwarding()
    }

    /** The connection is gone (or about to be rebuilt). The ringing phone keeps ringing until it times out. */
    fun onLinkDown() {
        resetNotifications()
        music.stop()
        refreshForwarding()
        hub.handler.removeCallbacks(resetFindWatch)
        setFindWatch(FindWatchUi.Idle)
    }

    /** The switches or the system access changed; [duties] is what is effective now. */
    fun onDutiesChanged(duties: DutyMask) {
        if (!duties.findPhone) alarm.stop()
    }

    private fun resetNotifications() {
        subscribed = false
        ids.clear()
        changes.clear()
    }

    private fun refreshForwarding() {
        forwarding = subscribed && link?.connectionDuties?.notifications == true
    }

    // ---- notifications (from the listener service) ----

    /** Any thread. */
    fun onPosted(n: NotificationSnapshot) {
        hub.handler.post { posted(n) }
    }

    /** Any thread. */
    fun onRemoved(key: String) {
        hub.handler.post { removed(key) }
    }

    private fun posted(n: NotificationSnapshot) {
        val link = link ?: return
        if (!forwarding) return
        val excluded = hub.state.value.settings.phone.excludedApps
        if (NotificationPolicy.decide(n, ownPackage, excluded) != Decision.Forward) return
        val watch = NotificationMapper.toWatch(n, ids.idFor(n.key, n.packageName))
        if (!changes.changed(n.key, watch.title, watch.body)) return
        link.sendDuty(
            Input.PostNotification(
                PhoneNotification(
                    id = watch.id.toUInt(),
                    appId = watch.appId,
                    appName = watch.appName,
                    category = when (watch.category) {
                        WatchCategory.Other -> NotificationCategory.OTHER
                        WatchCategory.Email -> NotificationCategory.EMAIL
                        WatchCategory.Message -> NotificationCategory.MESSAGE
                        WatchCategory.Social -> NotificationCategory.SOCIAL
                        WatchCategory.Location -> NotificationCategory.LOCATION
                    },
                    title = watch.title,
                    subtitle = watch.subtitle,
                    body = watch.body,
                    whenUnixS = watch.whenUnixS,
                ),
            ),
        )
    }

    private fun removed(key: String) {
        val id = ids.forget(key) ?: return
        changes.forget(key)
        if (forwarding) link?.sendDuty(Input.RemoveNotification(id.toUInt()))
    }

    // ---- events from the watch (WatchLink.handleEvent, on the handler) ----

    fun onWatchEvent(event: Event) {
        when (event) {
            is Event.NotificationSubscription -> {
                subscribed = event.enabled
                if (!event.enabled) {
                    ids.clear()
                    changes.clear()
                }
                refreshForwarding()
                hub.log.info(if (event.enabled) "The watch shows the phone's notifications" else "The watch stopped showing notifications")
            }
            is Event.NotificationAction -> onNotificationAction(event.id.toLong(), event.action)
            is Event.FindPhoneStart -> {
                hub.log.info("The watch asks the phone to ring")
                alarm.start(FindPhoneRules.ringSeconds(event.durationS.toInt()))
            }
            is Event.FindPhoneStop -> {
                hub.log.info("The watch stopped the ringing")
                alarm.stop()
            }
            is Event.MusicCommand -> music.onCommand(
                when (event.command) {
                    MusicCommand.TOGGLE_PLAY_PAUSE -> WatchMusicCommand.TogglePlayPause
                    MusicCommand.NEXT -> WatchMusicCommand.Next
                    MusicCommand.PREVIOUS -> WatchMusicCommand.Previous
                    MusicCommand.VOLUME_UP -> WatchMusicCommand.VolumeUp
                    MusicCommand.VOLUME_DOWN -> WatchMusicCommand.VolumeDown
                    MusicCommand.PLAY -> WatchMusicCommand.Play
                    MusicCommand.PAUSE -> WatchMusicCommand.Pause
                    MusicCommand.SKIP_FORWARD -> WatchMusicCommand.SkipForward
                    MusicCommand.SKIP_BACKWARD -> WatchMusicCommand.SkipBackward
                },
            )
            is Event.MusicRequested -> music.pushAll()
            is Event.FindWatch -> onFindWatch(event.state)
            else -> Unit
        }
    }

    private fun onNotificationAction(id: Long, action: NotificationAction) {
        val entry = ids.lookup(id) ?: return
        when (action) {
            is NotificationAction.Dismiss -> NimragNotificationListener.cancel(entry.key)
            is NotificationAction.Block -> {
                setExcluded(entry.packageName, true)
                hub.log.info("An app was excluded from the watch's notifications (Block on the watch). Undo it under Phone features")
            }
            else -> hub.log.info("The watch asked for a notification action we do not do yet")
        }
    }

    // ---- find my watch ----

    /** The owner asked the watch to ring (`start`) or to stop. */
    fun findWatch(start: Boolean) {
        val link = link
        if (link == null || !link.isReady()) {
            setFindWatch(FindWatchUi.Failed)
            return
        }
        hub.handler.removeCallbacks(resetFindWatch)
        setFindWatch(if (start) FindWatchUi.Asking else FindWatchUi.Idle)
        link.sendDuty(Input.FindWatch(start))
    }

    private fun onFindWatch(state: FindWatchState) {
        hub.handler.removeCallbacks(resetFindWatch)
        when (state) {
            FindWatchState.NOT_SUPPORTED -> setFindWatch(FindWatchUi.NotSupported)
            FindWatchState.RINGING -> {
                setFindWatch(FindWatchUi.Ringing)
                // The watch rings for a minute at most; do not show a stale "ringing".
                hub.handler.postDelayed(resetFindWatch, FIND_WATCH_SHOWN_MS)
            }
            FindWatchState.STOPPED, FindWatchState.FOUND_ON_WATCH -> setFindWatch(FindWatchUi.Idle)
            FindWatchState.FAILED -> setFindWatch(FindWatchUi.Failed)
        }
    }

    private fun setFindWatch(state: FindWatchUi) {
        hub.update { it.copy(phone = it.phone.copy(findWatch = state)) }
    }

    // ---- from the UI ----

    /** "Found it" in the app or on the notification. Any thread. */
    fun stopFindPhone() = alarm.stop()

    /** Keep (or stop keeping) one app's notifications on the phone. Any thread. */
    fun setExcluded(packageName: String, excluded: Boolean) {
        synchronized(this) {
            val now = hub.settings.excludedApps
            val next = if (excluded) now + packageName else now - packageName
            if (next != now) hub.settings.excludedApps = next
        }
        hub.update { it.copy(settings = hub.settings.toState()) }
    }

    private companion object {
        const val FIND_WATCH_SHOWN_MS = 60_000L
    }
}
