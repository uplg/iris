package studio.kahn.iris.tv.ui.screens

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import studio.kahn.iris.tv.ui.components.LoadingState

/**
 * The old series page (`series/{followId}`): a follow's id is its collection's id, so it is a
 * straight move to the title's page, as on the web.
 */
@Composable
fun SeriesScreen(followId: String, onOpenCollection: (collectionId: String) -> Unit) {
    LaunchedEffect(followId) { onOpenCollection(followId) }
    LoadingState(label = "Opening the series…")
}
