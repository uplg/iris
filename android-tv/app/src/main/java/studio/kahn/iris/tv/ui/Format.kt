package studio.kahn.iris.tv.ui

import java.util.Locale

/**
 * A size as the web app writes it (`@iris/api` `formatSize`): binary steps,
 * one decimal under 100, so both clients show the same number for a file.
 */
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
