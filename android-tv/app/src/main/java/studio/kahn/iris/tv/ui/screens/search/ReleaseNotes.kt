package studio.kahn.iris.tv.ui.screens.search

import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import studio.kahn.iris.tv.data.DescriptionFormat

/**
 * A release's notes as the tracker wrote them, in the format it declares
 * (web `release/bbcode.ts`, `html.ts`), turned into styled text the TV draws
 * itself: bold, italic, underline and the structure (paragraphs, list items,
 * table cells) are kept; the tracker's colors, sizes and fonts are dropped so
 * the text stays readable on our ground; images, scripts and links' targets
 * never load (a TV cannot follow them).
 */
fun releaseNotes(source: String, format: DescriptionFormat?): AnnotatedString = when (format) {
    DescriptionFormat.html -> htmlNotes(source)
    DescriptionFormat.plain -> AnnotatedString(tidy(source.replace(CRLF, "\n")))
    DescriptionFormat.bbcode, null -> bbcodeNotes(source)
}

private val CRLF = Regex("\r\n?")
private val BLANK_RUNS = Regex("\n[ \t]*\n([ \t]*\n)+")

private fun tidy(text: String): String = text.replace(BLANK_RUNS, "\n\n").trim()

/** One node of tracker BBCode. Unknown or unbalanced tags stay as text: nothing written is lost. */
sealed interface BBNode {
    data class Text(val value: String) : BBNode
    data class Tag(val name: String, val arg: String?, val children: List<BBNode>) : BBNode
}

// `[b]`, `[/b]`, `[color=#3d85c6]`, `[img scale=30%]` (extra attributes read and dropped)
private val TAG = Regex("""\[(/?)([a-zA-Z]+)(?:=([^\] ]*))?(?:\s+[^\]]*)?]""")

fun parseBBCode(input: String): List<BBNode> {
    class Frame(val name: String, val arg: String?, val children: MutableList<BBNode> = mutableListOf())
    val stack = ArrayDeque<Frame>().apply { addLast(Frame("", null)) }
    var cursor = 0
    for (m in TAG.findAll(input)) {
        val start = m.range.first
        if (start > cursor) stack.last().children += BBNode.Text(input.substring(cursor, start))
        cursor = m.range.last + 1
        val name = m.groupValues[2].lowercase()
        if (m.groupValues[1] != "/") {
            stack.addLast(Frame(name, m.groupValues[3].ifEmpty { null }))
            continue
        }
        val at = stack.indexOfLast { it.name == name }
        if (at <= 0) {
            stack.last().children += BBNode.Text(m.value)
            continue
        }
        // Tags left open inside the one closed end with it.
        while (stack.size > at) {
            val f = stack.removeLast()
            stack.last().children += BBNode.Tag(f.name, f.arg, f.children)
        }
    }
    if (cursor < input.length) stack.last().children += BBNode.Text(input.substring(cursor))
    // Never closed: the opening tag as text, its content kept.
    while (stack.size > 1) {
        val open = stack.removeLast()
        stack.last().children += BBNode.Text("[${open.name}${open.arg?.let { "=$it" }.orEmpty()}]")
        stack.last().children += open.children
    }
    return stack.single().children
}

private val SEPARATOR_GLYPHS = Regex("[━—–·•⋯]")
private val BB_TAGS = Regex("""\[[^]]+]""")

/** Lines drawn with separator glyphs (━━━, ···) are decoration, not text. */
fun stripSeparators(input: String): String = input.lines().filter { line ->
    val bare = line.replace(BB_TAGS, "").trim()
    bare.isEmpty() || SEPARATOR_GLYPHS.findAll(bare).count().toDouble() / bare.length < 0.5
}.joinToString("\n")

private val BOLD = SpanStyle(fontWeight = FontWeight.SemiBold)
private val ITALIC = SpanStyle(fontStyle = FontStyle.Italic)
private val UNDERLINE = SpanStyle(textDecoration = TextDecoration.Underline)
private val STRIKE = SpanStyle(textDecoration = TextDecoration.LineThrough)

fun bbcodeNotes(source: String): AnnotatedString {
    val nodes = parseBBCode(stripSeparators(source.replace(CRLF, "\n")).replace("[*]", "\n• "))
    val built = buildAnnotatedString {
        fun render(list: List<BBNode>) {
            for (node in list) {
                when (node) {
                    is BBNode.Text -> append(node.value)
                    is BBNode.Tag -> when (node.name) {
                        "b" -> withStyle(BOLD) { render(node.children) }
                        "i" -> withStyle(ITALIC) { render(node.children) }
                        "u" -> withStyle(UNDERLINE) { render(node.children) }
                        "s", "strike" -> withStyle(STRIKE) { render(node.children) }
                        "img" -> Unit
                        "url" -> if (node.children.isEmpty()) append(node.arg.orEmpty()) else render(node.children)
                        "table" -> render(node.children.filter { it is BBNode.Tag && it.name == "tr" })
                        "tr" -> {
                            val cells = node.children.filter { it is BBNode.Tag && (it.name == "td" || it.name == "th") }
                            cells.forEachIndexed { i, cell ->
                                if (i > 0) append(" · ")
                                render((cell as BBNode.Tag).children)
                            }
                            append("\n")
                        }
                        else -> render(node.children)
                    }
                }
            }
        }
        render(nodes)
    }
    return built.trimmed()
}

/** Collapses blank runs and trims, keeping the styles where they were. */
private fun AnnotatedString.trimmed(): AnnotatedString {
    val out = StringBuilder()
    val map = IntArray(text.length + 1)
    var newlines = 0
    for ((i, c) in text.withIndex()) {
        map[i] = out.length
        if (c == '\n') {
            newlines++
            if (newlines <= 2) out.append(c)
        } else {
            if (c != ' ' && c != '\t') newlines = 0
            out.append(c)
        }
    }
    map[text.length] = out.length
    val lead = out.indexOfFirst { !it.isWhitespace() }.let { if (it < 0) out.length else it }
    val tail = out.indexOfLast { !it.isWhitespace() } + 1
    if (tail <= lead) return AnnotatedString("")
    return buildAnnotatedString {
        append(out.substring(lead, tail))
        for (span in spanStyles) {
            val start = (map[span.start] - lead).coerceIn(0, tail - lead)
            val end = (map[span.end] - lead).coerceIn(0, tail - lead)
            if (end > start) addStyle(span.item, start, end)
        }
    }
}

private val HTML_DROP = Regex("""<(script|style|iframe|noscript|template)\b[^>]*>.*?</\1\s*>""", setOf(RegexOption.IGNORE_CASE, RegexOption.DOT_MATCHES_ALL))
private val HTML_COMMENT = Regex("""<!--.*?-->""", RegexOption.DOT_MATCHES_ALL)
private val HTML_TAG = Regex("""<(/?)([a-zA-Z][a-zA-Z0-9]*)[^>]*?(/?)>""")
private val HTML_SPACE = Regex("""[ \t\r\n]+""")
private val BLOCKS = setOf("p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "table", "tbody", "thead", "blockquote", "pre", "tr")

fun htmlNotes(source: String): AnnotatedString {
    val cleaned = source.replace(HTML_COMMENT, "").replace(HTML_DROP, "")
    val built = buildAnnotatedString {
        val open = ArrayDeque<Pair<String, Int>>()
        var cellInRow = 0
        var cursor = 0
        var last = '\n'
        fun add(s: String) {
            if (s.isEmpty()) return
            append(s)
            last = s.last()
        }
        fun text(raw: String) {
            val t = decodeEntities(raw.replace(HTML_SPACE, " "))
            if (t.isBlank() && last == '\n') return
            add(t)
        }
        for (m in HTML_TAG.findAll(cleaned)) {
            text(cleaned.substring(cursor, m.range.first))
            cursor = m.range.last + 1
            val closing = m.groupValues[1] == "/"
            val name = m.groupValues[2].lowercase()
            val style = when (name) {
                "b", "strong", "h1", "h2", "h3", "h4", "h5", "h6", "th" -> BOLD
                "i", "em" -> ITALIC
                "u" -> UNDERLINE
                "s", "strike", "del" -> STRIKE
                else -> null
            }
            when {
                name == "br" -> add("\n")
                name == "li" && !closing -> add("\n• ")
                (name == "td" || name == "th") && !closing -> {
                    if (cellInRow > 0) add(" · ")
                    cellInRow++
                }
                name == "tr" -> {
                    cellInRow = 0
                    add("\n")
                }
                name in BLOCKS -> add(if (closing) "\n\n" else "\n")
            }
            if (style != null && m.groupValues[3] != "/") {
                if (!closing) {
                    open.addLast(name to pushStyle(style))
                } else {
                    val at = open.indexOfLast { it.first == name }
                    if (at >= 0) {
                        val index = open[at].second
                        while (open.size > at) open.removeLast()
                        pop(index)
                    }
                }
            }
        }
        text(cleaned.substring(cursor))
    }
    return built.trimmed()
}

private val ENTITY = Regex("""&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);""")
private val NAMED = mapOf(
    "amp" to "&", "lt" to "<", "gt" to ">", "quot" to "\"", "apos" to "'", "nbsp" to " ",
    "eacute" to "é", "egrave" to "è", "ecirc" to "ê", "agrave" to "à", "ccedil" to "ç", "ocirc" to "ô",
    "laquo" to "«", "raquo" to "»", "hellip" to "…", "mdash" to "—", "ndash" to "–", "copy" to "©",
)

fun decodeEntities(text: String): String = text.replace(ENTITY) { m ->
    val v = m.groupValues[1]
    when {
        v.startsWith("#x") -> v.substring(2).toIntOrNull(16)?.let(::codePoint)
        v.startsWith("#") -> v.substring(1).toIntOrNull()?.let(::codePoint)
        else -> NAMED[v.lowercase()]
    } ?: m.value
}

private fun codePoint(cp: Int): String? = if (Character.isValidCodePoint(cp)) String(Character.toChars(cp)) else null
