package dev.nimrag.app.engine.auth.logic

/**
 * A small strict JSON reader and string quoter for the few app API answers (no Android types,
 * so the unit tests run on the JVM; `org.json` is a stub there). Objects become
 * `Map<String, Any?>`, arrays `List<Any?>`, numbers `Double`, plus `String`, `Boolean`, null.
 * Anything malformed, nested deeper than [MAX_DEPTH] or longer than [MAX_LENGTH] throws
 * [IllegalArgumentException] (never with the text in the message: it may hold secrets).
 */
object MiniJson {
    const val MAX_DEPTH = 16
    const val MAX_LENGTH = 64 * 1024

    /** [maxLength] raises the limit for the one answer that is long on purpose (a list of files). */
    fun parse(text: String, maxLength: Int = MAX_LENGTH): Any? {
        require(text.length <= maxLength) { "JSON too long" }
        val reader = Reader(text)
        val value = reader.value(0)
        reader.skipSpace()
        require(reader.atEnd()) { "JSON has trailing data" }
        return value
    }

    /** The parsed text as an object, or null when it is not a JSON object (or not JSON at all). */
    @Suppress("UNCHECKED_CAST")
    fun parseObject(text: String): Map<String, Any?>? =
        try {
            parse(text) as? Map<String, Any?>
        } catch (_: IllegalArgumentException) {
            null
        }

    /** A JSON string literal: quotes, backslashes and control characters escaped. */
    fun quote(value: String): String = buildString {
        append('"')
        for (c in value) {
            when (c) {
                '"' -> append("\\\"")
                '\\' -> append("\\\\")
                '\n' -> append("\\n")
                '\r' -> append("\\r")
                '\t' -> append("\\t")
                else -> if (c < ' ') append("\\u%04x".format(c.code)) else append(c)
            }
        }
        append('"')
    }

    /** `{"a":"x","b":"y"}` from string pairs (the app only ever sends string fields). */
    fun obj(vararg fields: Pair<String, String>): String =
        fields.joinToString(",", "{", "}") { (k, v) -> "${quote(k)}:${quote(v)}" }

    private class Reader(private val s: String) {
        private var i = 0

        fun atEnd() = i >= s.length

        fun skipSpace() {
            while (i < s.length && s[i] in " \t\r\n") i++
        }

        fun value(depth: Int): Any? {
            require(depth <= MAX_DEPTH) { "JSON nested too deep" }
            skipSpace()
            require(i < s.length) { "JSON ended early" }
            return when (val c = s[i]) {
                '{' -> obj(depth)
                '[' -> arr(depth)
                '"' -> str()
                't' -> literal("true", true)
                'f' -> literal("false", false)
                'n' -> literal("null", null)
                else -> if (c == '-' || c in '0'..'9') num() else throw IllegalArgumentException("bad JSON")
            }
        }

        private fun literal(word: String, result: Any?): Any? {
            require(s.startsWith(word, i)) { "bad JSON literal" }
            i += word.length
            return result
        }

        private fun obj(depth: Int): Map<String, Any?> {
            i++ // {
            val out = LinkedHashMap<String, Any?>()
            skipSpace()
            if (peek() == '}') {
                i++
                return out
            }
            while (true) {
                skipSpace()
                require(peek() == '"') { "bad JSON object" }
                val key = str()
                skipSpace()
                require(peek() == ':') { "bad JSON object" }
                i++
                out[key] = value(depth + 1)
                skipSpace()
                when (peek()) {
                    ',' -> i++
                    '}' -> {
                        i++
                        return out
                    }
                    else -> throw IllegalArgumentException("bad JSON object")
                }
            }
        }

        private fun arr(depth: Int): List<Any?> {
            i++ // [
            val out = ArrayList<Any?>()
            skipSpace()
            if (peek() == ']') {
                i++
                return out
            }
            while (true) {
                out.add(value(depth + 1))
                skipSpace()
                when (peek()) {
                    ',' -> i++
                    ']' -> {
                        i++
                        return out
                    }
                    else -> throw IllegalArgumentException("bad JSON array")
                }
            }
        }

        private fun peek(): Char? = if (i < s.length) s[i] else null

        private fun str(): String {
            i++ // opening quote
            val sb = StringBuilder()
            while (true) {
                require(i < s.length) { "JSON string not closed" }
                val c = s[i++]
                when {
                    c == '"' -> return sb.toString()
                    c == '\\' -> {
                        require(i < s.length) { "bad JSON escape" }
                        when (val e = s[i++]) {
                            '"', '\\', '/' -> sb.append(e)
                            'b' -> sb.append('\b')
                            'f' -> sb.append('\u000c')
                            'n' -> sb.append('\n')
                            'r' -> sb.append('\r')
                            't' -> sb.append('\t')
                            'u' -> {
                                require(i + 4 <= s.length) { "bad JSON escape" }
                                sb.append(s.substring(i, i + 4).toInt(16).toChar())
                                i += 4
                            }
                            else -> throw IllegalArgumentException("bad JSON escape")
                        }
                    }
                    c < ' ' -> throw IllegalArgumentException("bad JSON string")
                    else -> sb.append(c)
                }
            }
        }

        private fun num(): Double {
            val start = i
            if (peek() == '-') i++
            while (i < s.length && (s[i] in '0'..'9' || s[i] in ".eE+-")) i++
            return s.substring(start, i).toDoubleOrNull() ?: throw IllegalArgumentException("bad JSON number")
        }
    }
}
