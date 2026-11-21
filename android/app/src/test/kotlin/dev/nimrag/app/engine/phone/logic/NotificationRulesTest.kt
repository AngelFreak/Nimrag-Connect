package dev.nimrag.app.engine.phone.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class NotificationRulesTest {
    private val own = "dev.nimrag.app"

    private fun snap(
        key: String = "0|org.example.chat|1|null|10",
        pkg: String = "org.example.chat",
        label: String = "Example Chat",
        category: String? = "msg",
        title: String = "Ann",
        text: String = "Are you coming?",
        bigText: String = "",
        subText: String = "",
        ongoing: Boolean = false,
        summary: Boolean = false,
        postTimeMs: Long = 1_800_000_123_000,
    ) = NotificationSnapshot(key, pkg, label, category, title, text, bigText, subText, postTimeMs, ongoing, summary)

    // ---- policy ----

    @Test
    fun `an ordinary notification is forwarded`() {
        assertEquals(Decision.Forward, NotificationPolicy.decide(snap(), own, emptySet()))
    }

    @Test
    fun `Nimrag itself is never forwarded, whatever the exclusion list says`() {
        assertEquals(Decision.Skip(SkipReason.OwnApp), NotificationPolicy.decide(snap(pkg = own), own, emptySet()))
    }

    @Test
    fun `excluded apps stay on the phone, the others still go`() {
        val excluded = setOf("org.example.chat")
        assertEquals(Decision.Skip(SkipReason.Excluded), NotificationPolicy.decide(snap(), own, excluded))
        assertEquals(Decision.Forward, NotificationPolicy.decide(snap(pkg = "org.example.mail"), own, excluded))
    }

    @Test
    fun `group summaries and ongoing notifications are skipped, a call is kept`() {
        assertEquals(Decision.Skip(SkipReason.GroupSummary), NotificationPolicy.decide(snap(summary = true), own, emptySet()))
        assertEquals(Decision.Skip(SkipReason.Ongoing), NotificationPolicy.decide(snap(ongoing = true, category = "transport"), own, emptySet()))
        assertEquals(Decision.Skip(SkipReason.Ongoing), NotificationPolicy.decide(snap(ongoing = true, category = null), own, emptySet()))
        assertEquals(Decision.Forward, NotificationPolicy.decide(snap(ongoing = true, category = "call"), own, emptySet()))
    }

    @Test
    fun `a notification without any text is skipped`() {
        val empty = snap(title = " ", text = "", bigText = "\n")
        assertEquals(Decision.Skip(SkipReason.Empty), NotificationPolicy.decide(empty, own, emptySet()))
        assertEquals(Decision.Forward, NotificationPolicy.decide(snap(title = "", text = "x"), own, emptySet()))
        assertEquals(Decision.Forward, NotificationPolicy.decide(snap(title = "", text = "", bigText = "x"), own, emptySet()))
    }

    // ---- mapping ----

    @Test
    fun `categories follow the Android category, then well-known apps`() {
        assertEquals(WatchCategory.Email, NotificationMapper.category("email", "x"))
        assertEquals(WatchCategory.Message, NotificationMapper.category("msg", "x"))
        assertEquals(WatchCategory.Social, NotificationMapper.category("social", "x"))
        assertEquals(WatchCategory.Location, NotificationMapper.category("navigation", "x"))
        assertEquals(WatchCategory.Other, NotificationMapper.category("promo", "com.whatsapp"))
        assertEquals(WatchCategory.Message, NotificationMapper.category(null, "com.whatsapp"))
        assertEquals(WatchCategory.Email, NotificationMapper.category(null, "com.google.android.gm"))
        assertEquals(WatchCategory.Other, NotificationMapper.category(null, "org.unknown"))
    }

    @Test
    fun `the watch gets the long text when there is one`() {
        val w = NotificationMapper.toWatch(snap(text = "short", bigText = "the whole message", subText = "Group"), 7)
        assertEquals(7L, w.id)
        assertEquals("org.example.chat", w.appId)
        assertEquals("Example Chat", w.appName)
        assertEquals(WatchCategory.Message, w.category)
        assertEquals("Ann", w.title)
        assertEquals("Group", w.subtitle)
        assertEquals("the whole message", w.body)
        assertEquals(1_800_000_123L, w.whenUnixS)
    }

    @Test
    fun `a missing title falls back to the app name, a missing label to the package`() {
        assertEquals("Example Chat", NotificationMapper.toWatch(snap(title = ""), 1).title)
        val noLabel = NotificationMapper.toWatch(snap(label = "", title = ""), 1)
        assertEquals("org.example.chat", noLabel.title)
        assertEquals("org.example.chat", noLabel.appName)
    }

    @Test
    fun `long text is cut and never in the middle of a surrogate pair`() {
        val emoji = "😀" // one code point, two chars
        val body = "a".repeat(NotificationMapper.MAX_BODY_CHARS - 1) + emoji + "tail"
        val cut = NotificationMapper.toWatch(snap(text = body), 1).body
        assertEquals(NotificationMapper.MAX_BODY_CHARS - 1, cut.length)
        assertTrue(cut.all { it == 'a' })
        val fits = "a".repeat(NotificationMapper.MAX_BODY_CHARS - 2) + emoji
        assertEquals(NotificationMapper.MAX_BODY_CHARS, NotificationMapper.toWatch(snap(text = fits), 1).body.length)
        assertEquals("abc", NotificationMapper.cut("abc", 3))
    }

    @Test
    fun `a negative post time does not make a negative date`() {
        assertEquals(0L, NotificationMapper.toWatch(snap(postTimeMs = -5), 1).whenUnixS)
    }

    @Test
    fun `printing a notification shows none of its content`() {
        val n = snap(title = "Secret title", text = "Secret body", bigText = "Secret long body")
        assertFalse(n.toString().contains("Secret"))
        assertFalse(n.toString().contains("example"))
        val w = NotificationMapper.toWatch(n, 3)
        assertFalse(w.toString().contains("Secret"))
        assertFalse(w.toString().contains("example"))
    }

    // ---- ids ----

    @Test
    fun `an update keeps its id, a new notification gets the next one`() {
        val ids = NotificationIds()
        val a = ids.idFor("a", "pkg.a")
        val b = ids.idFor("b", "pkg.b")
        assertEquals(1L, a)
        assertEquals(2L, b)
        assertEquals(a, ids.idFor("a", "pkg.a"))
        assertEquals(2, ids.size)
    }

    @Test
    fun `the way back finds the key and the package`() {
        val ids = NotificationIds()
        val id = ids.idFor("key-1", "pkg.a")
        val entry = ids.lookup(id)!!
        assertEquals("key-1", entry.key)
        assertEquals("pkg.a", entry.packageName)
        assertNull(ids.lookup(999))
    }

    @Test
    fun `forgetting returns the id once and frees the entry`() {
        val ids = NotificationIds()
        val id = ids.idFor("k", "p")
        assertEquals(id, ids.forget("k"))
        assertNull(ids.forget("k"))
        assertNull(ids.lookup(id))
        // a notification that comes back is new for the watch
        assertTrue(ids.idFor("k", "p") != id)
    }

    @Test
    fun `the oldest entry goes when more than the capacity are known`() {
        val ids = NotificationIds(capacity = 3)
        val first = ids.idFor("1", "p")
        ids.idFor("2", "p")
        ids.idFor("3", "p")
        ids.idFor("4", "p")
        assertEquals(3, ids.size)
        assertNull(ids.lookup(first))
        assertNull(ids.forget("1"))
    }

    @Test
    fun `clear forgets everything`() {
        val ids = NotificationIds()
        ids.idFor("k", "p")
        ids.clear()
        assertEquals(0, ids.size)
        assertNull(ids.forget("k"))
    }

    // ---- change filter ----

    @Test
    fun `the same text is sent once, a changed text again`() {
        val f = ChangeFilter()
        assertTrue(f.changed("k", "Ann", "Hello"))
        assertFalse(f.changed("k", "Ann", "Hello"))
        assertTrue(f.changed("k", "Ann", "Hello again"))
        assertTrue(f.changed("other", "Ann", "Hello again"))
        assertFalse(f.changed("k", "Ann", "Hello again"))
    }

    @Test
    fun `a notification that was removed is new when it returns`() {
        val f = ChangeFilter()
        f.changed("k", "Ann", "Hello")
        f.forget("k")
        assertTrue(f.changed("k", "Ann", "Hello"))
        f.clear()
        assertTrue(f.changed("k", "Ann", "Hello"))
    }

    @Test
    fun `the filter is bounded`() {
        val f = ChangeFilter(capacity = 2)
        f.changed("1", "t", "b")
        f.changed("2", "t", "b")
        f.changed("3", "t", "b")
        // "1" was pushed out, so it counts as new
        assertTrue(f.changed("1", "t", "b"))
    }
}
