package studio.kahn.iris.tv.screenshot

import java.time.OffsetDateTime
import java.util.Locale
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveCountry
import studio.kahn.iris.tv.data.LiveNowNext
import studio.kahn.iris.tv.data.LiveProgramme
import studio.kahn.iris.tv.data.LiveSearchResult
import studio.kahn.iris.tv.ui.screens.live.LiveGuide
import studio.kahn.iris.tv.ui.screens.live.LiveResults
import studio.kahn.iris.tv.ui.screens.live.LiveTvContent
import studio.kahn.iris.tv.ui.screens.live.LiveTvUi
import studio.kahn.iris.tv.ui.state.Loadable

/**
 * Live TV's channel list (web `/live`) with the catalogue's real shape: some sixty
 * countries, France's free-to-air with a guide, a country of three hundred channels
 * without one, channels that may be blocked or are not answering, logos missing.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = TV_QUALIFIERS)
class LiveScreenshots {
    @get:Rule
    val shots = IrisScreenshotRule()

    private val now = OffsetDateTime.parse("2026-10-06T20:40:00Z")
    private val clock = { t: OffsetDateTime -> String.format(Locale.ROOT, "%02d:%02d", t.hour, t.minute) }

    private val countries = listOf(
        "ad" to "Andorra", "ae" to "United Arab Emirates", "af" to "Afghanistan", "al" to "Albania", "ar" to "Argentina",
        "at" to "Austria", "au" to "Australia", "be" to "Belgium", "bg" to "Bulgaria", "br" to "Brazil", "ca" to "Canada",
        "ch" to "Switzerland", "cl" to "Chile", "cn" to "China", "co" to "Colombia", "cz" to "Czech Republic",
        "de" to "Germany", "dk" to "Denmark", "dz" to "Algeria", "eg" to "Egypt", "es" to "Spain", "fi" to "Finland",
        "fr" to "France", "gb" to "United Kingdom", "gr" to "Greece", "hk" to "Hong Kong", "hr" to "Croatia",
        "hu" to "Hungary", "id" to "Indonesia", "ie" to "Ireland", "il" to "Israel", "in" to "India", "iq" to "Iraq",
        "it" to "Italy", "jp" to "Japan", "kr" to "South Korea", "lb" to "Lebanon", "lu" to "Luxembourg",
        "ma" to "Morocco", "mx" to "Mexico", "nl" to "Netherlands", "no" to "Norway", "nz" to "New Zealand",
        "pe" to "Peru", "ph" to "Philippines", "pk" to "Pakistan", "pl" to "Poland", "pt" to "Portugal",
        "re" to "Réunion", "ro" to "Romania", "rs" to "Serbia", "ru" to "Russia", "sa" to "Saudi Arabia",
        "se" to "Sweden", "sn" to "Senegal", "th" to "Thailand", "tn" to "Tunisia", "tr" to "Turkey",
        "ua" to "Ukraine", "us" to "United States", "vn" to "Vietnam",
    ).mapIndexed { i, (code, name) ->
        LiveCountry(code = code, flag = code.uppercase(), name = name, channelCount = if (code == "fr") 40 else if (code == "us") 300 else 4 + (i * 37) % 260)
    }

    private fun channel(
        id: String,
        name: String,
        tnt: Int? = null,
        cat: String? = null,
        geo: Boolean = false,
        dead: Boolean = false,
        part: Boolean = false,
    ) = LiveChannel(
        categories = listOfNotNull(cat),
        geoBlocked = geo,
        id = id,
        name = name,
        not247 = part,
        tntNumber = tnt,
        unreachable = dead.takeIf { it },
    )

    private val tntNames = listOf(
        "TF1", "France 2", "France 3", "France 4", "France 5", "M6", "Arte", "LCP", "W9", "TMC", "TFX", "Gulli", "BFM TV",
        "CNews", "LCI", "franceinfo", "CStar", "T18", "NOVO19", "TF1 Séries Films", "L’Équipe", "6ter", "RMC Story",
        "RMC Découverte", "Chérie 25",
    )

    private val france = tntNames.mapIndexed { i, n -> channel("t$i", n, tnt = i + 1) } + listOf(
        channel("f24", "France 24", cat = "News"),
        channel("euronews", "Euronews", cat = "News", geo = true),
        channel("rfi", "RFI TV", cat = "News"),
        channel("africanews", "Africanews", cat = "News"),
        channel("tv5", "TV5Monde", cat = "General"),
        channel("mezzo", "Mezzo", cat = "Music", part = true),
        channel("trace", "Trace Urban", cat = "Music", dead = true),
        channel("nrj", "NRJ Hits", cat = "Music"),
        channel("museum", "Museum TV", cat = "Culture"),
        channel("equidia", "Équidia", cat = "Sports"),
        channel("nanar", "Ciné Nanar", cat = "Movies"),
        channel("canal", "Canal+ Clair", cat = "Entertainment", geo = true, dead = true),
        channel("bfmb", "BFM Business", cat = "Business"),
        channel("grenoble", "Télé Grenoble", cat = "Local"),
        channel("kto", "KTO", cat = "Religious"),
    )

    private val titles = listOf(
        "Le journal de 20 h", "Koh-Lanta, la légende", "Des chiffres et des lettres", "C dans l’air",
        "Plus belle la vie, encore plus belle", "Le Grand Échiquier", "Les Simpson", "Questions pour un champion",
        "Envoyé spécial", "Un si grand soleil", "Thalassa", "Météo",
    )

    private val guide = LiveGuide(
        france.filter { it.unreachable != true && it.id != "africanews" && it.id != "grenoble" }.mapIndexed { i, c ->
            val start = now.minusMinutes(5L + (i * 17) % 70)
            val stop = now.plusMinutes(5L + (i * 23) % 80)
            c.id to LiveNowNext(
                channelId = c.id,
                now = LiveProgramme(start = start, stop = stop, title = titles[i % titles.size]),
                next = LiveProgramme(start = stop, stop = stop.plusMinutes(45), title = titles[(i + 5) % titles.size]),
            )
        }.toMap(),
        now.toInstant().toEpochMilli(),
    )

    private val categories = listOf("News", "Sports", "Entertainment", "Movies", "Kids", "Music", "Religious", "Documentary", "Shop", "Local", "Lifestyle", "Weather")
    private val words = listOf("Channel", "TV", "Network", "Live", "Plus", "One", "Now", "Classic", "Prime", "Central")
    private val states = List(300) { i ->
        val cat = categories[(i * 7) % categories.size]
        channel("us$i", "$cat ${words[i % words.size]} ${i + 1}", cat = cat.takeIf { i % 13 != 0 }, geo = i % 17 == 0, dead = i % 29 == 0, part = i % 11 == 0)
    }

    private fun ui(
        country: String = "fr",
        channels: Loadable<List<LiveChannel>> = Loadable.Ready(france),
        guide: LiveGuide = this.guide,
        query: String = "",
        results: Loadable<LiveResults>? = null,
    ) = LiveTvUi(
        countries = countries,
        country = country,
        channels = channels,
        guide = guide,
        query = query,
        results = results,
        usual = countries.filter { it.code == "fr" || it.code == "us" || it.code == "be" }.sortedBy { listOf("fr", "us", "be").indexOf(it.code) },
    )

    private fun content(ui: LiveTvUi, picking: Boolean = false) = @androidx.compose.runtime.Composable {
        LiveTvContent(
            ui = ui,
            clock = clock,
            countryName = { code -> countries.firstOrNull { it.code == code }?.let { "${it.flag} ${it.name}" } ?: code },
            onQueryChange = {},
            onPickCountry = {},
            onOpen = { _, _ -> },
            onRetry = {},
            initiallyPicking = picking,
        )
    }

    @Test
    fun liveList() = shots.snapEverySize("live_list", content(ui()))

    @Test
    fun liveListWithoutGuide() = shots.snapEverySize(
        "live_list_noguide",
        content(ui(country = "us", channels = Loadable.Ready(states), guide = LiveGuide(readAtMs = 1L))),
    )

    @Test
    fun liveCountryPicker() = shots.snapEverySize("live_country_picker", content(ui(), picking = true))

    @Test
    fun liveEmptyCountry() = shots.snap("live_empty", content = content(ui(country = "ad", channels = Loadable.Ready(emptyList()))))

    @Test
    fun liveSearch() = shots.snap(
        "live_search",
        content = content(
            ui(
                query = "france",
                results = Loadable.Ready(
                    LiveResults(
                        "france",
                        listOf(
                            "fr" to listOf(LiveSearchResult(country = "fr", id = "france2", name = "France 2"), LiveSearchResult(country = "fr", id = "f24", name = "France 24")),
                            "be" to listOf(LiveSearchResult(country = "be", id = "f24", name = "France 24")),
                            "ch" to listOf(LiveSearchResult(country = "ch", id = "tv5", name = "France TV5 Suisse")),
                        ),
                    ),
                ),
            ),
        ),
    )
}
