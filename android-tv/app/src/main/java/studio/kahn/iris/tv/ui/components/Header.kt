package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.R
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** The five top-level destinations, in header order, with their visible names. */
enum class TopTab(val label: String) {
    Home("Home"),
    Search("Search"),
    Discover("Discover"),
    Library("Library"),
    LiveTv("Live TV"),
}

/**
 * The Iris lockup: the mark, then "Iris" in Borel (the only place Borel is
 * used). [fontSize] scales both; the default is the header's 34 px.
 */
@Composable
fun IrisWordmark(
    modifier: Modifier = Modifier,
    fontSize: TextUnit = IrisType.brand.fontSize,
) {
    val markSize = with(LocalDensity.current) { (fontSize * MARK_TO_TEXT).toDp() }
    val drop = with(LocalDensity.current) { (fontSize * BOREL_BASELINE_DROP).toDp() }
    Row(
        modifier.clearAndSetSemantics { contentDescription = "Iris" },
        horizontalArrangement = Arrangement.spacedBy(markSize * GAP_TO_MARK),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Image(painterResource(R.drawable.iris_mark), contentDescription = null, modifier = Modifier.size(markSize))
        Text(
            "Iris",
            style = IrisType.brand.copy(fontSize = fontSize, lineHeight = fontSize),
            color = IrisColor.ink,
            modifier = Modifier.offset(y = drop),
        )
    }
}

private const val MARK_TO_TEXT = 44f / 34f
private const val GAP_TO_MARK = 14f / 44f
// Borel sits high on its line box; the boards push it down 0.16 em.
private const val BOREL_BASELINE_DROP = 0.16f

/**
 * One top-level tab. [selected] = the screen you are on: accent words on the
 * accent wash, announced as selected. Focused, it fills with ink like every
 * control.
 */
@Composable
fun NavTab(
    text: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    FocusSurface(
        onClick = onClick,
        modifier = modifier
            .heightIn(min = IrisSize.controlSmall)
            .semantics {
                role = Role.Tab
                this.selected = selected
            },
        shape = IrisShape.pill,
        colors = FocusColors.Quiet.copy(
            container = if (selected) IrisColor.accentWash else Color.Transparent,
            content = if (selected) IrisColor.accent else IrisColor.inkMuted,
        ),
    ) {
        Box(Modifier.padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
            Text(text, style = IrisType.control, maxLines = 1)
        }
    }
}

/**
 * The page header of every top-level screen (TV.dc.html): the wordmark, the
 * five [TopTab]s, then the account (avatar initial + name). Place it at the
 * top of the safe area; the boards leave a 20 dp gap below it.
 *
 * Entering the header with the D-pad lands on the [current] tab
 * (focusRestorer falls back to it), then on whichever tab was focused last.
 * [onAccount] makes the account focusable (it opens Settings); null shows it
 * as plain words. [updateAvailable] marks it (a dot on the avatar and
 * "Update" after the name): Settings holds the update.
 */
@Composable
fun TvHeader(
    current: TopTab,
    onSelect: (TopTab) -> Unit,
    accountName: String?,
    modifier: Modifier = Modifier,
    onAccount: (() -> Unit)? = null,
    updateAvailable: Boolean = false,
) {
    val currentTab = remember { FocusRequester() }
    Row(
        modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IrisWordmark()
        Spacer(Modifier.width(IrisSpace.s6))
        Row(
            Modifier
                .focusRestorer(currentTab)
                .focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s1),
        ) {
            TopTab.entries.forEach { tab ->
                NavTab(
                    text = tab.label,
                    selected = tab == current,
                    onClick = { onSelect(tab) },
                    modifier = if (tab == current) Modifier.focusRequester(currentTab) else Modifier,
                )
            }
        }
        Spacer(Modifier.weight(1f))
        if (accountName != null) {
            if (onAccount != null) {
                FocusSurface(
                    onClick = onAccount,
                    modifier = Modifier.heightIn(min = IrisSize.controlSmall),
                    shape = IrisShape.pill,
                    colors = FocusColors.Quiet.copy(content = IrisColor.inkMuted),
                ) { focused ->
                    Account(
                        accountName,
                        focused,
                        updateAvailable,
                        Modifier
                            .padding(start = 3.dp, end = 12.dp)
                            .clearAndSetSemantics {
                                contentDescription = if (updateAvailable) {
                                    "Settings, update available, $accountName"
                                } else {
                                    "Settings, $accountName"
                                }
                            },
                    )
                }
            } else {
                Account(accountName, focused = false, update = false)
            }
        }
    }
}

@Composable
private fun Account(name: String, focused: Boolean, update: Boolean, modifier: Modifier = Modifier) {
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(IrisSize.avatar)) {
            Box(
                Modifier
                    .matchParentSize()
                    .background(IrisColor.accentWash, IrisShape.circle),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    name.trim().take(1).uppercase(),
                    style = IrisType.chip.copy(fontWeight = FontWeight.Bold),
                    color = IrisColor.accent,
                )
            }
            if (update) {
                Box(
                    Modifier
                        .align(Alignment.TopEnd)
                        .offset(x = 2.dp, y = (-2).dp)
                        .size(IrisSpace.s3)
                        .border(1.5.dp, IrisColor.ground, IrisShape.circle)
                        .padding(1.5.dp)
                        .background(IrisColor.accent, IrisShape.circle),
                )
            }
        }
        Text(
            name,
            style = IrisType.meta,
            color = if (focused) IrisColor.ground else IrisColor.inkMuted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        if (update) {
            Text(
                "Update",
                style = IrisType.meta.copy(fontWeight = FontWeight.Bold),
                color = if (focused) IrisColor.ground else IrisColor.accent,
                maxLines = 1,
            )
        }
    }
}
