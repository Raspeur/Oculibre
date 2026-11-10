package org.oculibre

import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import java.util.TreeMap

/**
 * Minimal JSON for the protocol (docs/protocol.typ §6), with no Android dependency so that the
 * JVM unit tests run it as is (org.json is only a stub there).
 *
 * Values: `Map<String, Any?>` (insertion order kept), `List<Any?>`, `String`, `Long` (integers),
 * `Double` (other numbers), `Boolean`, `null`.
 *
 * The writer produces the same bytes as serde_json on the host (compact, same escaping), so the
 * shared test vectors can be re-encoded byte for byte.
 */
object Json {
    class ParseException(message: String) : Exception(message)

    private const val MAX_DEPTH = 32

    // ------------------------------------------------------------------ parsing

    /** Parses UTF-8 bytes. Rejects invalid UTF-8, trailing garbage and nesting deeper than 32. */
    fun parse(bytes: ByteArray, offset: Int = 0, length: Int = bytes.size - offset): Any? {
        val text = try {
            Charsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(bytes, offset, length))
                .toString()
        } catch (e: CharacterCodingException) {
            throw ParseException("invalid UTF-8")
        }
        return parse(text)
    }

    fun parse(text: String): Any? {
        val p = Parser(text)
        p.skipWs()
        val v = p.value(0)
        p.skipWs()
        if (p.pos != text.length) p.fail("trailing characters")
        return v
    }

    private class Parser(val s: String) {
        var pos = 0

        fun fail(what: String): Nothing = throw ParseException("$what at offset $pos")

        fun skipWs() {
            while (pos < s.length) {
                val c = s[pos]
                if (c == ' ' || c == '\t' || c == '\n' || c == '\r') pos++ else break
            }
        }

        fun value(depth: Int): Any? {
            if (depth > MAX_DEPTH) fail("nesting too deep")
            if (pos >= s.length) fail("unexpected end")
            return when (s[pos]) {
                '{' -> obj(depth)
                '[' -> arr(depth)
                '"' -> str()
                't' -> literal("true", true)
                'f' -> literal("false", false)
                'n' -> literal("null", null)
                else -> number()
            }
        }

        fun literal(word: String, v: Any?): Any? {
            if (!s.startsWith(word, pos)) fail("invalid literal")
            pos += word.length
            return v
        }

        fun obj(depth: Int): Map<String, Any?> {
            val m = LinkedHashMap<String, Any?>()
            pos++ // {
            skipWs()
            if (pos < s.length && s[pos] == '}') { pos++; return m }
            while (true) {
                skipWs()
                if (pos >= s.length || s[pos] != '"') fail("expected key")
                val k = str()
                skipWs()
                if (pos >= s.length || s[pos] != ':') fail("expected ':'")
                pos++
                skipWs()
                m[k] = value(depth + 1) // duplicate keys: last one wins, like serde_json::Value
                skipWs()
                if (pos >= s.length) fail("unexpected end")
                when (s[pos++]) {
                    ',' -> continue
                    '}' -> return m
                    else -> { pos--; fail("expected ',' or '}'") }
                }
            }
        }

        fun arr(depth: Int): List<Any?> {
            val l = ArrayList<Any?>()
            pos++ // [
            skipWs()
            if (pos < s.length && s[pos] == ']') { pos++; return l }
            while (true) {
                skipWs()
                l.add(value(depth + 1))
                skipWs()
                if (pos >= s.length) fail("unexpected end")
                when (s[pos++]) {
                    ',' -> continue
                    ']' -> return l
                    else -> { pos--; fail("expected ',' or ']'") }
                }
            }
        }

        fun str(): String {
            pos++ // "
            val sb = StringBuilder()
            while (true) {
                if (pos >= s.length) fail("unterminated string")
                val c = s[pos++]
                when {
                    c == '"' -> return sb.toString()
                    c == '\\' -> {
                        if (pos >= s.length) fail("unterminated escape")
                        when (val e = s[pos++]) {
                            '"' -> sb.append('"')
                            '\\' -> sb.append('\\')
                            '/' -> sb.append('/')
                            'b' -> sb.append('\b')
                            'f' -> sb.append('\u000C')
                            'n' -> sb.append('\n')
                            'r' -> sb.append('\r')
                            't' -> sb.append('\t')
                            'u' -> {
                                if (pos + 4 > s.length) fail("bad \\u escape")
                                val code = s.substring(pos, pos + 4).toIntOrNull(16) ?: fail("bad \\u escape")
                                sb.append(code.toChar())
                                pos += 4
                            }
                            else -> { pos--; fail("bad escape '$e'") }
                        }
                    }
                    c < ' ' -> { pos--; fail("control character in string") }
                    else -> sb.append(c)
                }
            }
        }

        fun number(): Any {
            val start = pos
            if (pos < s.length && s[pos] == '-') pos++
            if (pos >= s.length) fail("invalid number")
            if (s[pos] == '0') {
                pos++
            } else if (s[pos] in '1'..'9') {
                while (pos < s.length && s[pos] in '0'..'9') pos++
            } else {
                fail("invalid value")
            }
            var integral = true
            if (pos < s.length && s[pos] == '.') {
                integral = false
                pos++
                val d = pos
                while (pos < s.length && s[pos] in '0'..'9') pos++
                if (pos == d) fail("invalid number")
            }
            if (pos < s.length && (s[pos] == 'e' || s[pos] == 'E')) {
                integral = false
                pos++
                if (pos < s.length && (s[pos] == '+' || s[pos] == '-')) pos++
                val d = pos
                while (pos < s.length && s[pos] in '0'..'9') pos++
                if (pos == d) fail("invalid number")
            }
            val text = s.substring(start, pos)
            if (integral) text.toLongOrNull()?.let { return it }
            return text.toDouble()
        }
    }

    // ------------------------------------------------------------------ writing

    fun write(value: Any?): String = StringBuilder().also { write(value, it) }.toString()

    fun writeBytes(value: Any?): ByteArray = write(value).toByteArray(Charsets.UTF_8)

    fun write(value: Any?, sb: StringBuilder) {
        when (value) {
            null -> sb.append("null")
            is String -> writeString(value, sb)
            is Boolean -> sb.append(if (value) "true" else "false")
            is Long, is Int, is Short, is Byte -> sb.append(value.toString())
            is Double -> writeDouble(value, sb)
            is Float -> writeDouble(value.toDouble(), sb)
            is Map<*, *> -> {
                sb.append('{')
                var first = true
                for ((k, v) in value) {
                    if (!first) sb.append(',')
                    first = false
                    writeString(k as String, sb)
                    sb.append(':')
                    write(v, sb)
                }
                sb.append('}')
            }
            is Iterable<*> -> {
                sb.append('[')
                var first = true
                for (v in value) {
                    if (!first) sb.append(',')
                    first = false
                    write(v, sb)
                }
                sb.append(']')
            }
            else -> throw IllegalArgumentException("not a JSON value: ${value::class.java}")
        }
    }

    private fun writeDouble(d: Double, sb: StringBuilder) {
        if (d.isNaN() || d.isInfinite()) { sb.append("null"); return } // as serde_json
        sb.append(d.toString().replace("E", "e"))
    }

    private val HEX = "0123456789abcdef"

    /** Same escaping as serde_json: `"`, `\`, and control characters; everything else as is. */
    private fun writeString(s: String, sb: StringBuilder) {
        sb.append('"')
        for (c in s) {
            when (c) {
                '"' -> sb.append("\\\"")
                '\\' -> sb.append("\\\\")
                '\n' -> sb.append("\\n")
                '\r' -> sb.append("\\r")
                '\t' -> sb.append("\\t")
                '\b' -> sb.append("\\b")
                '\u000C' -> sb.append("\\f")
                else -> if (c < ' ') {
                    sb.append("\\u00").append(HEX[c.code shr 4]).append(HEX[c.code and 15])
                } else {
                    sb.append(c)
                }
            }
        }
        sb.append('"')
    }

    /**
     * Free-form values (event data, command args and results) with object keys sorted, as the
     * host's serde_json does for them: keeps the bytes identical on both sides.
     */
    fun sorted(value: Any?): Any? = when (value) {
        is Map<*, *> -> TreeMap<String, Any?>().also { m ->
            for ((k, v) in value) m[k as String] = sorted(v)
        }
        is List<*> -> value.map { sorted(it) }
        else -> value
    }
}

/** Base64 (RFC 4648, with padding). `java.util.Base64` needs API 26 and `android.util.Base64` is not on the JVM. */
object Base64Codec {
    private const val ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"

    fun encode(b: ByteArray, offset: Int = 0, length: Int = b.size - offset): String {
        val sb = StringBuilder((length + 2) / 3 * 4)
        var i = offset
        val end = offset + length
        while (i + 3 <= end) {
            val n = (b[i].toInt() and 0xFF shl 16) or (b[i + 1].toInt() and 0xFF shl 8) or (b[i + 2].toInt() and 0xFF)
            sb.append(ALPHABET[n shr 18 and 63]).append(ALPHABET[n shr 12 and 63])
                .append(ALPHABET[n shr 6 and 63]).append(ALPHABET[n and 63])
            i += 3
        }
        when (end - i) {
            1 -> {
                val n = b[i].toInt() and 0xFF shl 16
                sb.append(ALPHABET[n shr 18 and 63]).append(ALPHABET[n shr 12 and 63]).append("==")
            }
            2 -> {
                val n = (b[i].toInt() and 0xFF shl 16) or (b[i + 1].toInt() and 0xFF shl 8)
                sb.append(ALPHABET[n shr 18 and 63]).append(ALPHABET[n shr 12 and 63])
                    .append(ALPHABET[n shr 6 and 63]).append('=')
            }
        }
        return sb.toString()
    }

    /** Strict decoder: returns null on any invalid character or length. */
    fun decode(s: String): ByteArray? {
        val t = s.trim()
        if (t.length % 4 != 0) return null
        val out = java.io.ByteArrayOutputStream(t.length / 4 * 3)
        var i = 0
        while (i < t.length) {
            val pad = if (i + 4 == t.length) t.substring(i).count { it == '=' } else 0
            if (pad > 2) return null
            var n = 0
            for (j in 0 until 4) {
                val c = t[i + j]
                val v = if (c == '=' && j >= 4 - pad) 0 else ALPHABET.indexOf(c)
                if (v < 0) return null
                n = n shl 6 or v
            }
            out.write(n shr 16 and 0xFF)
            if (pad < 2) out.write(n shr 8 and 0xFF)
            if (pad < 1) out.write(n and 0xFF)
            i += 4
        }
        return out.toByteArray()
    }
}
