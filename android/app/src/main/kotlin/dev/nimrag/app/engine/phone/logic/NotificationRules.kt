package dev.nimrag.app.engine.phone.logic

/**
 * What the notification listener hands over for one notification. Private: it lives in memory
 * only for as long as it takes to decide and forward, and [toString] shows none of it, so a
 * stray log line cannot leak it.
 */
class NotificationSnapshot(
    val key: String,
    val packageName: String,
    val appLabel: String,
    /** `Notification.category`, e.g. "msg" or "email"; null when the app set none. */
    val category: String?,
    val title: String,
    val text: String,
    val bigText: String,
    val subText: String,
    val postTimeMs: Long,
    val ongoing: Boolean,
    val groupSummary: Boolean,
) {
    override fun toString() = "NotificationSnapshot(…)"
}

enum class SkipReason { OwnApp, Excluded, GroupSummary, Ongoing, Empty }

sealed interface Decision {
    data object Forward : Decision
    data class Skip(val reason: SkipReason) : Decision
}

/** The kinds the watch tells apart (icon and sorting); see the protocol client's `NotificationCategory`. */
enum class WatchCategory { Other, Email, Message, Social, Location }

/** What goes to the watch for one notification (private, like [NotificationSnapshot]). */
class WatchNotification(
    val id: Long,
    val appId: String,
    val appName: String,
    val category: WatchCategory,
    val title: String,
    val subtitle: String,
    val body: String,
    val whenUnixS: Long,
) {
    override fun toString() = "WatchNotification(id=$id, category=$category)"
}

/**
 * Which notifications go to the watch. The default is everything except Nimrag itself, apps the
 * owner excluded, and noise that is not a message: group summaries (their children are sent
 * on their own), ongoing notifications (music players, navigation, running services; a call
 * stays) and notifications without any text.
 */
object NotificationPolicy {
    const val CATEGORY_CALL = "call"

    fun decide(n: NotificationSnapshot, ownPackage: String, excluded: Set<String>): Decision = when {
        n.packageName == ownPackage -> Decision.Skip(SkipReason.OwnApp)
        n.packageName in excluded -> Decision.Skip(SkipReason.Excluded)
        n.groupSummary -> Decision.Skip(SkipReason.GroupSummary)
        n.ongoing && n.category != CATEGORY_CALL -> Decision.Skip(SkipReason.Ongoing)
        n.title.isBlank() && n.text.isBlank() && n.bigText.isBlank() -> Decision.Skip(SkipReason.Empty)
        else -> Decision.Forward
    }
}

/** Android's notification in the watch's terms. */
object NotificationMapper {
    /** The watch shows little; the cap also keeps one huge notification from filling the link. */
    const val MAX_BODY_CHARS = 1000
    const val MAX_TITLE_CHARS = 200

    private val messagingApps = setOf(
        "com.whatsapp",
        "com.whatsapp.w4b",
        "org.thoughtcrime.securesms",
        "org.telegram.messenger",
        "com.google.android.apps.messaging",
        "com.android.mms",
        "com.facebook.orca",
        "com.discord",
        "com.Slack",
        "com.microsoft.teams",
        "ch.threema.app",
    )
    private val mailApps = setOf("com.google.android.gm", "com.fsck.k9", "com.microsoft.office.outlook", "me.proton.android.mail")
    private val socialApps = setOf("com.instagram.android", "com.twitter.android", "com.facebook.katana", "com.linkedin.android", "org.joinmastodon.android")

    /** `Notification.category` first; a few well-known apps when the app set none. */
    fun category(androidCategory: String?, packageName: String): WatchCategory = when (androidCategory) {
        "email" -> WatchCategory.Email
        "msg" -> WatchCategory.Message
        "social" -> WatchCategory.Social
        "navigation" -> WatchCategory.Location
        null -> when (packageName) {
            in messagingApps -> WatchCategory.Message
            in mailApps -> WatchCategory.Email
            in socialApps -> WatchCategory.Social
            else -> WatchCategory.Other
        }
        else -> WatchCategory.Other
    }

    fun toWatch(n: NotificationSnapshot, id: Long): WatchNotification {
        val appName = n.appLabel.ifBlank { n.packageName }
        val title = n.title.trim().ifEmpty { appName }
        // The long text is the whole message; the short one is what the shade shows.
        val body = n.bigText.trim().ifEmpty { n.text.trim() }
        return WatchNotification(
            id = id,
            appId = n.packageName,
            appName = appName,
            category = category(n.category, n.packageName),
            title = cut(title, MAX_TITLE_CHARS),
            subtitle = cut(n.subText.trim(), MAX_TITLE_CHARS),
            body = cut(body, MAX_BODY_CHARS),
            whenUnixS = n.postTimeMs.coerceAtLeast(0) / 1000,
        )
    }

    /** At most [max] chars, never splitting a surrogate pair. */
    fun cut(text: String, max: Int): String {
        if (text.length <= max) return text
        var end = max
        if (Character.isHighSurrogate(text[end - 1])) end -= 1
        return text.substring(0, end)
    }
}

/**
 * The numbers the watch knows notifications by (a 31-bit counter, never reused until it wraps)
 * and the way back to the phone's notification key, for "dismiss" and "block" on the watch.
 * Bounded: the oldest entry goes when more than [capacity] are known.
 */
class NotificationIds(private val capacity: Int = 256) {
    class Entry(val key: String, val packageName: String)

    private val byKey = LinkedHashMap<String, Long>()
    private val byId = HashMap<Long, Entry>()
    private var next = 1L

    val size: Int get() = byKey.size

    /** The id of [key]; a new one when the notification is new, the same one when it is updated. */
    fun idFor(key: String, packageName: String): Long {
        byKey[key]?.let { return it }
        val id = next
        next = if (next >= MAX_ID) 1 else next + 1
        byKey[key] = id
        byId[id] = Entry(key, packageName)
        while (byKey.size > capacity) {
            val oldest = byKey.entries.first()
            byKey.remove(oldest.key)
            byId.remove(oldest.value)
        }
        return id
    }

    fun lookup(id: Long): Entry? = byId[id]

    /** The notification is gone: its id when it was known. */
    fun forget(key: String): Long? {
        val id = byKey.remove(key) ?: return null
        byId.remove(id)
        return id
    }

    fun clear() {
        byKey.clear()
        byId.clear()
    }

    private companion object {
        const val MAX_ID = 0x7FFF_FFFFL
    }
}

/**
 * Apps re-post a notification for every progress tick; the watch would buzz each time. A
 * notification goes out again only when its title or text changed. Only a hash is kept.
 */
class ChangeFilter(private val capacity: Int = 256) {
    private val seen = LinkedHashMap<String, Int>()

    /** True when [key] is new or its content differs from the last one that went out. */
    fun changed(key: String, title: String, body: String): Boolean {
        val hash = 31 * title.hashCode() + body.hashCode()
        val before = seen.remove(key)
        seen[key] = hash
        while (seen.size > capacity) seen.remove(seen.keys.first())
        return before != hash
    }

    fun forget(key: String) {
        seen.remove(key)
    }

    fun clear() = seen.clear()
}
