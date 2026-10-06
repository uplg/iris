package studio.kahn.iris.tv.data

import kotlinx.coroutines.CancellationException

/**
 * [block]'s value, or null when it failed: for a read or a write whose failure changes nothing
 * the person sees (a convenience, a decoration). Unlike `runCatching`, cancellation propagates:
 * a coroutine cancelled mid-call stops there instead of carrying on with a null. A value that
 * is shown goes through `load()` instead, so its failure is said.
 */
inline fun <T> bestEffort(block: () -> T): T? =
    try {
        block()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        null
    }
