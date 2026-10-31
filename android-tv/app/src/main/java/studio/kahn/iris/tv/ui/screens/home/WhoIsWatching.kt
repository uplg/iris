package studio.kahn.iris.tv.ui.screens.home

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.data.ProfileView
import studio.kahn.iris.tv.ui.components.FocusColors
import studio.kahn.iris.tv.ui.components.FocusSurface
import studio.kahn.iris.tv.ui.components.Mascot
import studio.kahn.iris.tv.ui.components.Notice
import studio.kahn.iris.tv.ui.components.NoticeLine
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

private val MASCOT = 128.dp

/** Who is watching, once per launch of the app when the account has several profiles, each by their mascot; Back stays on the current one (web `ProfileGate`). */
@Composable
fun WhoIsWatching(profiles: List<ProfileView>, notice: Notice?, onPick: (ProfileView) -> Unit) {
    val first = remember { FocusRequester() }
    val current = profiles.firstOrNull { it.current } ?: profiles.first()
    LaunchedEffect(Unit) { runCatching { first.requestFocus() } }
    BackHandler { onPick(current) }
    Box(
        Modifier
            .fillMaxSize()
            .background(IrisColor.ground),
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(IrisSpace.s7)) {
            Text("Who’s watching?", style = IrisType.title, color = IrisColor.ink)
            FlowRow(Modifier.focusGroup(), horizontalArrangement = Arrangement.spacedBy(IrisSpace.s6)) {
                profiles.forEach { p ->
                    FocusSurface(
                        onClick = { onPick(p) },
                        modifier = if (p == current) Modifier.focusRequester(first) else Modifier,
                        shape = IrisShape.card,
                        colors = FocusColors.Quiet,
                    ) { focused ->
                        Column(
                            Modifier.padding(IrisSpace.s4),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
                        ) {
                            Mascot(p.name, MASCOT)
                            Text(p.name, style = IrisType.bodyStrong, color = if (focused) IrisColor.ground else IrisColor.ink)
                        }
                    }
                }
            }
            NoticeLine(notice)
        }
    }
}
