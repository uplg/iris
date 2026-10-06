package studio.kahn.iris.tv.ui.theme

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** Fraunces sets short titles at 20 sp and up, nothing else. */
@RunWith(RobolectricTestRunner::class)
class TypeRuleTest {
    private val fraunces = IrisType.title.fontFamily

    @Test
    fun aShortTitleKeepsFraunces() {
        assertEquals(fraunces, IrisType.titleFor("Severance", IrisType.title).fontFamily)
    }

    @Test
    fun figuresAndSentencesTurnToCalSansAtTheSameSize() {
        for (text in listOf("Severance · 2022", "Severance · Season 2, complete", "8f14e45fceea167a5a36dedd4bea2543", "A".repeat(60))) {
            val style = IrisType.titleFor(text, IrisType.title)
            assertEquals(text, CalSans, style.fontFamily)
            assertEquals(IrisType.title.fontSize, style.fontSize)
        }
    }

    @Test
    fun everyFrauncesStyleIsAtLeast20Sp() {
        val styles = listOf(
            IrisType.hero, IrisType.headline, IrisType.title, IrisType.page, IrisType.stageTitle,
            IrisType.artTitle, IrisType.artTitleStill, IrisType.artTitleSmall, IrisType.panel, IrisType.section, IrisType.group,
        )
        styles.filter { it.fontFamily != CalSans }.forEach { assertTrue(it.fontSize.value >= 20f) }
        assertNotEquals("the fallback poster title is Cal Sans", fraunces, IrisType.artTitle.fontFamily)
    }

    @Test
    fun theFloors() {
        assertTrue(IrisType.reading.fontSize.value >= 12f)
        assertTrue(IrisType.metaSmall.fontSize.value >= 10f)
        assertTrue(IrisType.key.fontSize.value >= 10f)
    }
}
