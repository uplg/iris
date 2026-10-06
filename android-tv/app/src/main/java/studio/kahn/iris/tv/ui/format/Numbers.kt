package studio.kahn.iris.tv.ui.format

import java.util.Locale

/*
 * The words every screen uses, written once, as the web app writes them
 * (`packages/iris-api/src/format.ts`, `web/src/lib/history/words.ts`,
 * `web/src/lib/language.ts`): the same number reads the same on both clients.
 */

/** A size with binary steps, one decimal under 100: `512 B`, `1.5 KB`, `12.4 GB`; `?` when negative. */
fun formatSize(bytes: Long): String {
    if (bytes < 0) return "?"
    val units = listOf("B", "KB", "MB", "GB", "TB")
    var n = bytes.toDouble()
    var i = 0
    while (n >= 1024 && i < units.size - 1) {
        n /= 1024
        i++
    }
    val pattern = if (n >= 100 || i == 0) "%.0f %s" else "%.1f %s"
    return String.format(Locale.ROOT, pattern, n, units[i])
}

/** A transfer speed: `6.1 MB/s`. */
fun formatSpeed(bytesPerSecond: Long): String = "${formatSize(bytesPerSecond)}/s"

/** A count with its noun: `1 download`, `3 downloads`. */
fun plural(n: Number, one: String, many: String = "${one}s"): String = "$n ${if (n.toLong() == 1L) one else many}"

/** A share 0 to 100 as people read it: `42%`. */
fun percent(fraction0to100: Double): String = "${Math.round(fraction0to100)}%"
