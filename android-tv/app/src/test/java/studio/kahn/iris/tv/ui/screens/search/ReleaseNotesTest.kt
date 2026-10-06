package studio.kahn.iris.tv.ui.screens.search

import androidx.compose.ui.text.font.FontWeight
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.DescriptionFormat

class ReleaseNotesTest {
    @Test
    fun bbcodeKeepsTheTextAndDropsTheTrackersPalette() {
        val notes = releaseNotes(
            "[center][size=150][color=#3d85c6][b]Title[/b][/color][/size][/center]\n━━━━━━━━\nBody [i]text[/i].\n[img]https://x/y.png[/img]",
            DescriptionFormat.bbcode,
        )
        assertEquals("Title\nBody text.", notes.text)
        val bold = notes.spanStyles.single { it.item.fontWeight == FontWeight.SemiBold }
        assertEquals("Title", notes.text.substring(bold.start, bold.end))
    }

    @Test
    fun unbalancedTagsStayAsText() {
        assertEquals(listOf(BBNode.Text("a "), BBNode.Text("[/b]"), BBNode.Text(" c")), parseBBCode("a [/b] c"))
        assertEquals("[b]open", bbcodeNotes("[b]open").text)
    }

    @Test
    fun tablesAndListsBecomeLines() {
        assertEquals("Source · WEB\nCodec · x264", bbcodeNotes("[table][tr][td]Source[/td][td]WEB[/td][/tr]\n[tr][td]Codec[/td][td]x264[/td][/tr][/table]").text)
        assertEquals("• one\n• two", bbcodeNotes("[list][*]one[*]two[/list]").text)
    }

    @Test
    fun htmlIsMadeSafeText() {
        val notes = htmlNotes(
            "<h2 style=\"color:red\">Notes</h2><p>Un film <b>culte</b> &amp; rare.</p><script>alert(1)</script>" +
                "<ul><li>One</li><li>Two</li></ul><img src=x onerror=alert(1)><p>Fin&nbsp;!</p>",
        )
        assertEquals("Notes\n\nUn film culte & rare.\n\n• One\n• Two\n\nFin !", notes.text)
        assertTrue(notes.spanStyles.any { notes.text.substring(it.start, it.end) == "culte" })
    }

    @Test
    fun plainTextIsTidied() {
        assertEquals("a\n\nb", releaseNotes("a\r\n\r\n\r\n\r\nb  ", DescriptionFormat.plain).text)
        assertEquals("é &#xZZ;", decodeEntities("&#233; &#xZZ;"))
    }
}
