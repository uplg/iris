package studio.kahn.iris.tv.ui.state

import java.io.IOException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.ui.components.Notice

@OptIn(ExperimentalCoroutinesApi::class)
class BusyActionsTest {
    @Test
    fun anActionIsBusyUntilTheServerAnswers() = runTest {
        val actions = BusyActions(this)
        val answer = CompletableDeferred<Unit>()
        var done = false
        assertTrue(actions.runSaying("delete:a", onSuccess = { done = true }) { answer.await(); "Deleted." })
        runCurrent()
        assertEquals(setOf("delete:a"), actions.state.value.busy)
        assertFalse(actions.run("delete:a") { null })
        assertTrue(actions.run("pause:b") { null })
        answer.complete(Unit)
        runCurrent()
        assertTrue(done)
        assertEquals(Notice("Deleted.", failed = false), actions.state.value.notice)
        assertTrue(actions.state.value.busy.isEmpty())
    }

    @Test
    fun aFailureIsSaidAndNotDone() = runTest {
        val actions = BusyActions(this)
        var done = false
        actions.run("hide:x", onSuccess = { done = true }) { throw IOException("offline") }
        runCurrent()
        assertFalse(done)
        assertTrue(actions.state.value.notice?.failed == true)
    }

    @Test
    fun oneAtATimeRefusesAnyOther() = runTest {
        val actions = BusyActions(this, oneAtATime = true)
        val answer = CompletableDeferred<Unit>()
        actions.run("play-next") { answer.await(); null }
        runCurrent()
        assertTrue(actions.refuses("watched:1"))
        assertFalse(actions.run("watched:1") { null })
        assertEquals("play-next", actions.state.value.busyKey)
        answer.complete(Unit)
        runCurrent()
        assertFalse(actions.refuses("watched:1"))
    }
}
