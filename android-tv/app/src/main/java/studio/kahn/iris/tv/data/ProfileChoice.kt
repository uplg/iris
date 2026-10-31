package studio.kahn.iris.tv.data

/**
 * Whether this launch of the app asked who is watching, and how many times the profile changed:
 * the home starts over when it did, wherever it was changed from.
 */
object ProfileChoice {
    @Volatile var asked = false

    @Volatile var changes = 0
        private set

    fun changed() {
        asked = true
        changes++
    }

    fun forget() {
        asked = false
    }
}
