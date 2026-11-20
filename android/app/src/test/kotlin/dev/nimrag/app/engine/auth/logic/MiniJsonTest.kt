package dev.nimrag.app.engine.auth.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Test

class MiniJsonTest {
    @Test
    fun readsTheLoginAnswer() {
        val json = MiniJson.parseObject(
            """{"me":{"authenticated":true,"username":"anna","role":"user","timezone":"Europe/Copenhagen","sample":false},"device_id":12,"token":"nrg_x","device_secret":"nrd_y"}""",
        )!!
        assertEquals(12.0, json["device_id"])
        assertEquals("nrg_x", json["token"])
        assertEquals("anna", (json["me"] as Map<*, *>)["username"])
        assertEquals(false, (json["me"] as Map<*, *>)["sample"])
    }

    @Test
    fun readsEscapesAndUnicode() {
        assertEquals("a\"b\\c/d\nå", MiniJson.parse("\"a\\\"b\\\\c\\/d\\n\\u00e5\""))
    }

    @Test
    fun readsArraysNullsAndNumbers() {
        assertEquals(listOf(1.0, -2.5, 1000.0, null, true), MiniJson.parse("[1, -2.5, 1e3, null, true]"))
        assertEquals(emptyMap<String, Any?>(), MiniJson.parse(" { } "))
    }

    @Test
    fun malformedTextIsNotAnObject() {
        for (bad in listOf("", "{", "[1,]", "{\"a\":}", "{'a':1}", "nope", "{\"a\":1} x", "\"unterminated", "{\"a\":\"\u0001\"}")) {
            assertNull(MiniJson.parseObject(bad), bad)
        }
        assertNull(MiniJson.parseObject("[1]"))
        assertNull(MiniJson.parseObject("\"text\""))
    }

    @Test
    fun deepNestingAndHugeInputAreRefused() {
        assertNull(MiniJson.parseObject("[".repeat(100) + "]".repeat(100)))
        assertNull(MiniJson.parseObject("{\"a\":\"" + "x".repeat(MiniJson.MAX_LENGTH) + "\"}"))
    }

    @Test
    fun theErrorNeverRepeatsTheText() {
        val secret = "nrd_supersecretvalue"
        val e = assertThrows(IllegalArgumentException::class.java) { MiniJson.parse("{\"a\":$secret}") }
        assertFalse(e.message.orEmpty().contains(secret))
    }

    @Test
    fun quotesWhatMustBeEscaped() {
        assertEquals("\"a\\\"b\\\\c\\n\\t\\u0001\"", MiniJson.quote("a\"b\\c\n\t\u0001"))
    }

    @Test
    fun builtObjectsParseBack() {
        val text = MiniJson.obj("username" to "an\"na", "password" to "p\\w\nd", "code" to "123456")
        val back = MiniJson.parseObject(text)!!
        assertEquals("an\"na", back["username"])
        assertEquals("p\\w\nd", back["password"])
        assertEquals("123456", back["code"])
    }
}
