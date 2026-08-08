//! SCENE-style filename parser.
//!
//! Releases on private trackers (and most public ones) follow a tight
//! convention:
//!
//! ```text
//! Show.Name.S01E02.1080p.WEB-DL.x264-GROUP.mkv
//! Show.Name.2024.1080p.BluRay.x265-GROUP.mkv
//! ```
//!
//! Parsing this lets us derive `(series_title, season, episode)` for
//! TV files even when the indexer didn't set `tmdb_id`, and group
//! single-episode torrents into one library entry per show.
//!
//! Anime patterns (`[GROUP] Show Name - 02 [1080p]`) are NOT yet
//! supported — not bad for an MVP since most anime releases come
//! season-packed and don't trigger the multi-torrent-grouping path
//! anyway. Add support when a real-world case demands it.

/// Parsed pieces of a SCENE-style filename. Only `title` is required;
/// `season`/`episode` are present for TV releases, absent for movies.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub title: String,
    pub year: Option<u16>,
    /// Set for TV releases. When `season.is_some()`, `episode` is also
    /// set; the parser never produces one without the other.
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub quality: Option<String>,
    pub source: Option<String>,
    pub group: Option<String>,
    /// Absolute episode number from an anime release
    /// (`[Group] Title - NN [tags]`). Only set when the strict anime
    /// branch matched — never for SXXEXX or year-tagged releases.
    pub absolute_episode: Option<u32>,
}

impl Parsed {
    pub fn is_tv(&self) -> bool {
        self.season.is_some()
    }

    pub fn is_movie(&self) -> bool {
        !self.is_tv()
    }

    /// Lowercased, whitespace-collapsed, punctuation-stripped form of
    /// the title. Used as the dedup key for SCENE-grouped collections
    /// (the `parsed_title_normalized` column in the `collections`
    /// table). Same input from different release groups normalises to
    /// the same string so they all land in one collection.
    pub fn normalized_key(&self) -> String {
        normalize_title(&self.title)
    }

    /// Identity key for collection grouping. TV uses just the
    /// title with a trailing year stripped — TV releases sometimes
    /// inline the show's premiere year before the SE marker
    /// (`Lucky.Luke.1991.S01E01.…`), which would otherwise produce
    /// "lucky luke 1991" and prevent it from joining a follow whose
    /// user-typed name is just "Lucky Luke". Movies append the
    /// year so remakes (Dune 1984 vs Dune 2021) stay in distinct
    /// collections. The kind is passed in because the same
    /// `Parsed` can be classified TV-or-movie at the collection
    /// level.
    pub fn collection_key(&self, is_tv: bool) -> String {
        let title = normalize_title(&self.title);
        if is_tv {
            strip_trailing_year(&title)
        } else {
            match self.year {
                Some(y) if !title.is_empty() => format!("{title} {y}"),
                _ => title,
            }
        }
    }

    /// Anime-aware variant of [`Self::collection_key`]. An anime and a
    /// live-action show can share a title (the anime *One Piece* vs the
    /// Netflix live-action *One Piece*) yet are different entities that
    /// must never land in the same collection. We carry the distinction
    /// *inside* the normalised key as an `anime:` prefix:
    /// `normalize_title` strips every non-alphanumeric, so a real title
    /// can never produce a `:` — the prefix is collision-proof and needs
    /// no change to the `(parsed_title_normalized, kind)` unique index.
    /// Only applies to TV (movies keep year-based disambiguation).
    pub fn collection_key_kind(&self, is_tv: bool, is_anime: bool) -> String {
        let base = self.collection_key(is_tv);
        if is_anime && is_tv && !base.is_empty() {
            format!("anime:{base}")
        } else {
            base
        }
    }

    /// Display variant of [`Self::collection_key`] — preserves casing
    /// and renders the year as `Title (YYYY)` for movies. Used for
    /// the `display_title` column on the `collections` table.
    pub fn display_with_year(&self, is_tv: bool) -> String {
        if is_tv {
            self.title.clone()
        } else {
            match self.year {
                Some(y) => format!("{} ({y})", self.title),
                None => self.title.clone(),
            }
        }
    }
}

/// Canonical SCENE identity for a series — `normalize_title` plus
/// trailing-year strip. Same shape as
/// `Parsed::collection_key(true)` so a follow's `normalized_name`
/// (computed from the user-facing display title) collides cleanly
/// with the SCENE-derived `parsed_title_normalized` on
/// `collections`. Idempotent.
pub fn series_key(s: &str) -> String {
    strip_trailing_year(&normalize_title(s))
}

/// Episode numbers at or below this are treated as ordinary
/// per-season numbering; anything above it (under a single season) is
/// the anime "fleuve" convention where the *absolute* episode number is
/// crammed into a fake `S01` (`One Piece S01E1156`). A real broadcast
/// season essentially never exceeds ~100 episodes, so this cleanly
/// separates `S01E1156` (absolute) from `S02E05` / `S01E61` (seasonal).
pub const ABSOLUTE_EPISODE_THRESHOLD: u32 = 100;

/// Known anime release / fansub group tokens. Their presence as a
/// bounded token in a release name is a strong, anime-specific signal
/// (these groups only ever ship anime). Uppercase to match
/// [`has_token`], which is called against the upper-cased name.
const ANIME_GROUP_TOKENS: &[&str] = &[
    "TSUNDERE-RAWS",
    "ERAI-RAWS",
    "SUBSPLEASE",
    "HORRIBLESUBS",
    "ANIME-TIME",
    "BEATRICE-RAWS",
    "TENRAI-SENSEI",
    "KAWAIIKA-RAWS",
    "NANDESUKA",
    "JUDAS",
    "EMBER",
    "FOXEN",
    "COMMIE",
    "YAMEII",
    "NANAMI",
    "CLEO",
    "ASW",
];

/// Anime-specific release-FORMAT tokens — `OAV`/`OVA` ("Original
/// Animation/Video Video"), `ONA` ("Original Net Animation"). Unlike a high
/// episode count or a `VOSTFR` tag (both also occur on live-action
/// releases), these terms are anime-exclusive — no live-action distributor
/// ships an "OVA". Catches a season-0 special (`Show.S00E01.(OAV).VOSTFR…
/// -GROUP`) released by a plain scene group: no bracket fansub prefix, no
/// known fansub-group token, and the `season == 1`-gated fleuve check below
/// never fires for season 0 — without this, such a special's `is_anime`
/// flag disagrees with the main series' (set from its season-1 episodes),
/// which changes `collection_key_kind`'s `anime:` prefix and splits the
/// special into its own orphan collection instead of joining the show.
const ANIME_FORMAT_TOKENS: &[&str] = &["OAV", "OVA", "ONA"];

/// Best-effort, **offline** "is this an anime release?" classifier, run
/// at ingest to decide collection identity (see
/// [`Parsed::collection_key_kind`]). Deliberately conservative — it
/// only fires on anime-specific naming, never on a raw high episode
/// count alone, so a long-running Western show isn't mis-split:
///
///   * a known fansub group token (`-Tsundere-Raws`, `[Erai-raws]`, …),
///   * the bracketed `[Group] …` fansub shape,
///   * an anime-format token (`OAV`/`OVA`/`ONA` — see
///     [`ANIME_FORMAT_TOKENS`]), or
///   * the fleuve pattern (`S01` with an episode number above the
///     [`ABSOLUTE_EPISODE_THRESHOLD`]) **corroborated** by a `VOSTFR`
///     subbing tag.
///
/// AniList / TMDB confirmation happens asynchronously after ingest and
/// only ever *strengthens* the flag (fills `anilist_id`); it never
/// flips it back, because the flag is baked into the collection key the
/// moment the row is created.
pub fn looks_like_anime_release(name: &str, season: Option<u32>, episode: Option<u32>) -> bool {
    let upper = name.to_ascii_uppercase();
    if ANIME_GROUP_TOKENS.iter().any(|g| has_token(&upper, g)) {
        return true;
    }
    if name.trim_start().starts_with('[') {
        return true;
    }
    if ANIME_FORMAT_TOKENS.iter().any(|t| has_token(&upper, t)) {
        return true;
    }
    let fleuve = season == Some(1) && episode.is_some_and(|e| e > ABSOLUTE_EPISODE_THRESHOLD);
    fleuve && has_token(&upper, "VOSTFR")
}

/// Derive the *absolute* episode number for an anime release, or `None`
/// when the release uses ordinary seasonal numbering. Only meaningful
/// for collections already classified anime — callers gate on that.
///
///   * `[Group] Title - NN` → the bracket-form absolute (always).
///   * `S01E1156` (fleuve) → the episode number, but only when it
///     exceeds [`ABSOLUTE_EPISODE_THRESHOLD`], so a genuine seasonal
///     anime (`Demon.Slayer.S02E05`) stays seasonal.
pub fn absolute_from_parsed(p: &Parsed) -> Option<u32> {
    if let Some(abs) = p.absolute_episode {
        return Some(abs);
    }
    match (p.season, p.episode) {
        (Some(_), Some(e)) if e > ABSOLUTE_EPISODE_THRESHOLD => Some(e),
        _ => None,
    }
}

/// SCENE-aware ordering for raw torrent file lists. Compares two
/// file paths by:
///
/// 1. Extracted `(season, episode)` when both names carry an
///    SCENE-style marker (`SxxExx`) — sorts in natural episode
///    order regardless of file size. Without this, a 2.0 GB
///    `S02E03` lands above 1.8 GB `S02E02` lands above 1.9 GB
///    `S02E01` and the user has to hunt for episode 1.
/// 2. Files with markers sort BEFORE files without — so a TV
///    pack's main episodes sit on top, with extras / featurettes
///    afterwards.
/// 3. Fallback: case-insensitive lexicographic compare on the
///    basename (no natural-numeric handling at this level — Rust's
///    stdlib doesn't ship one, and SCENE filenames almost always
///    have leading zeros so plain `cmp` is fine for them).
///
/// Compares basenames only — the directory prefix changes for
/// multi-disc packs but the basename carries the SCENE marker.
pub fn compare_video_files(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let base_a = a.rsplit('/').next().unwrap_or(a);
    let base_b = b.rsplit('/').next().unwrap_or(b);
    let key_a = parse(base_a).and_then(|p| Some((p.season?, p.episode?)));
    let key_b = parse(base_b).and_then(|p| Some((p.season?, p.episode?)));
    match (key_a, key_b) {
        (Some((sa, ea)), Some((sb, eb))) => sa.cmp(&sb).then_with(|| ea.cmp(&eb)),
        // SE-marked files come first; bonus / extras drop to the end.
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => base_a
            .to_ascii_lowercase()
            .cmp(&base_b.to_ascii_lowercase()),
    }
}

/// Coarse language signal extracted from a SCENE release name. Used
/// by the collections scheduler + grab fallback to avoid mixing
/// English releases (typical Seedpool / mainline UNIT3D output) into
/// a collection whose existing episodes are French — the user
/// flagged this after watching it happen on an anime series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Language {
    /// Explicit French marker (`VOSTFR`, `VFF`, `VFQ`, `FRENCH`, etc).
    French,
    /// Bilingual / multi-audio release. Acceptable for both French
    /// and English preferences — the user can pick the audio track
    /// at playback time.
    Multi,
    /// Explicit English marker.
    English,
    /// No language marker detected. In practice almost always means
    /// English (English releases ship language-tag-less; FR releases
    /// almost always tag), so the matcher treats `Unknown` as
    /// "compatible with English".
    #[default]
    Unknown,
}

/// Detect a coarse language signal from a SCENE-style release
/// name. Order matters: a release tagged `MULTi.VFF` should
/// resolve to [`Language::Multi`] (broader compat), not French
/// alone — Multi grabs satisfy both pref sides, so claiming Multi
/// is the higher-information answer.
pub fn detect_language(title: &str) -> Language {
    let upper = title.to_ascii_uppercase();
    // Multi-audio first — matches both FR and EN preferences when
    // resolved through `Language::satisfies`.
    if has_token(&upper, "MULTI") {
        return Language::Multi;
    }
    // French markers. `SUBFRENCH` / `TRUEFRENCH` are subsets of
    // FRENCH but we check them explicitly so the broad FRENCH
    // match doesn't lose them in the token-boundary check.
    // VF2 / FR2 indicates both VFF (français de France) and VFQ
    // (français québécois) dubs are present — still a French
    // release from the matcher's point of view.
    for marker in [
        "VOSTFR",
        "SUBFRENCH",
        "TRUEFRENCH",
        "FRENCH",
        "VFF",
        "VFQ",
        "VFI",
        "VF2",
        "FR2",
        "VOQ",
        "VOF",
    ] {
        if has_token(&upper, marker) {
            return Language::French;
        }
    }
    // Explicit English tag — rare but seen on UNIT3D forks that
    // bother emitting one. Bare titles fall through to Unknown.
    if has_token(&upper, "ENGLISH") {
        return Language::English;
    }
    Language::Unknown
}

impl Language {
    /// `true` when a release tagged as `self` is acceptable for a
    /// collection whose preferred language is `preferred`. Multi
    /// releases satisfy both sides; Unknown releases count as
    /// English (the wild-west default for indexer output). When
    /// `preferred` itself is Unknown the matcher accepts anything
    /// — applies to the first ingest of a series, where no
    /// preference has been established yet.
    pub fn satisfies(self, preferred: Language) -> bool {
        matches!(
            (preferred, self),
            (Language::Unknown, _)
                | (_, Language::Multi)
                | (Language::French, Language::French)
                | (Language::English, Language::English | Language::Unknown),
        )
    }

    /// Stable string form used as the value of
    /// `available_episodes.language` and shipped to clients on
    /// `SearchResult.language` / `AvailableEpisodeEntry.language`.
    /// Lowercase for grep-friendliness; matches the corresponding
    /// `from_str` round-trip.
    pub fn as_str(self) -> &'static str {
        match self {
            Language::French => "french",
            Language::English => "english",
            Language::Multi => "multi",
            Language::Unknown => "unknown",
        }
    }

    /// Inverse of [`Self::as_str`] — accepts the canonical string
    /// AND a couple of historical aliases. Anything unrecognised
    /// collapses to `Unknown` so the matcher stays lenient on
    /// stale or future-format DB rows. Kept as an inherent method
    /// (not `std::str::FromStr`) so callers can stay infallible —
    /// every input maps to a valid variant.
    pub fn parse_tag(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "french" | "fr" | "vf" => Language::French,
            "english" | "en" | "vo" => Language::English,
            "multi" => Language::Multi,
            _ => Language::Unknown,
        }
    }
}

/// Coarse video-codec signal extracted from a SCENE release name. Used
/// by the Android TV client to badge / deprioritise (never hide — see
/// `feedback_no_hide_bad_data`) results the connected box can't
/// hardware-decode. `Unknown` is the safe default for titles that omit a
/// codec tag entirely (common) — absence of evidence isn't evidence of
/// `Av1`, so an `Unknown` result is never penalised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Codec {
    H264,
    Hevc,
    Av1,
    Vp9,
    #[default]
    Unknown,
}

/// Detect a coarse codec signal from a SCENE-style release name. Order
/// matters: check the more specific/exotic tags first so a title doesn't
/// fall through to the wrong bucket — there's no overlap risk here (each
/// tag set is disjoint) but keeping the same shape as
/// [`detect_language`] makes the two easy to read side by side.
pub fn detect_codec(title: &str) -> Codec {
    let upper = title.to_ascii_uppercase();
    if has_token(&upper, "AV1") {
        return Codec::Av1;
    }
    for marker in ["HEVC", "X265", "H265", "H.265"] {
        if has_token(&upper, marker) {
            return Codec::Hevc;
        }
    }
    for marker in ["X264", "H264", "H.264", "AVC"] {
        if has_token(&upper, marker) {
            return Codec::H264;
        }
    }
    if has_token(&upper, "VP9") {
        return Codec::Vp9;
    }
    Codec::Unknown
}

impl Codec {
    /// Stable string form shipped on `SearchResult.codec`. Lowercase for
    /// grep-friendliness, matches `Language::as_str`'s convention.
    pub fn as_str(self) -> &'static str {
        match self {
            Codec::H264 => "h264",
            Codec::Hevc => "hevc",
            Codec::Av1 => "av1",
            Codec::Vp9 => "vp9",
            Codec::Unknown => "unknown",
        }
    }
}

/// Video-source classes that mean "filmed off a screen / pre-retail" —
/// the releases household users grab by accident and regret (a CAM of a
/// fresh theatrical movie looks like any other result on a card). Only
/// dubious sources are modelled; clean sources (WEB-DL, `BluRay`…) stay
/// out of scope because absence of a marker proves nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DubiousSource {
    /// Camera pointed at a theater screen, camera audio.
    Cam,
    /// Theater camera video + direct audio feed (`TS` / `TELESYNC`).
    Telesync,
    /// Digitized from a film reel before retail (`TC` / `TELECINE`).
    Telecine,
    /// Pre-release screener copy (`SCR` / `DVDSCR` / `SCREENER`).
    Screener,
}

impl DubiousSource {
    /// Short badge form, matches the SCENE vocabulary users see in the
    /// release name itself.
    pub fn label(self) -> &'static str {
        match self {
            DubiousSource::Cam => "CAM",
            DubiousSource::Telesync => "TS",
            DubiousSource::Telecine => "TC",
            DubiousSource::Screener => "SCREENER",
        }
    }

    /// One-sentence English explanation for detail views. No trailing
    /// period — callers punctuate.
    pub fn explanation(self) -> &'static str {
        match self {
            DubiousSource::Cam => {
                "this release was filmed with a camera pointed at a theater screen \
                 — expect poor image and sound"
            }
            DubiousSource::Telesync => {
                "this release is a telesync: a theater camera recording with a direct \
                 audio feed — the sound is decent but the image is poor"
            }
            DubiousSource::Telecine => {
                "this release is a telecine, digitized from a film reel before retail \
                 — quality varies and is usually well below a proper rip"
            }
            DubiousSource::Screener => {
                "this release is a pre-release screener copy — often watermarked and \
                 quality-limited"
            }
        }
    }
}

/// Detect a dubious video source from a SCENE-style release name.
/// `None` means "no dubious marker found", not "verified clean".
///
/// Two tiers because the short tokens collide with real titles ("Cam"
/// is a 2018 film, `TS`/`TC` can be initials): unambiguous markers
/// (`HDCAM`, `TELESYNC`, …) match anywhere, while `CAM` / `TS` / `TC`
/// only count in the attribute tail — after the first year / `SxxEyy` /
/// language / resolution token, which is where SCENE names stop being
/// title words. A no-marker name never triggers the ambiguous tier.
pub fn detect_dubious_source(title: &str) -> Option<DubiousSource> {
    use DubiousSource::{Cam, Screener, Telecine, Telesync};
    let upper = title.to_ascii_uppercase();
    for (marker, src) in [
        ("HDCAM", Cam),
        ("CAMRIP", Cam),
        ("HQCAM", Cam),
        ("TELESYNC", Telesync),
        ("HDTS", Telesync),
        ("TSRIP", Telesync),
        ("TELECINE", Telecine),
        ("HDTC", Telecine),
        ("SCREENER", Screener),
        ("DVDSCR", Screener),
        ("BDSCR", Screener),
        ("WEBSCR", Screener),
    ] {
        if has_token(&upper, marker) {
            return Some(src);
        }
    }
    let tail = attribute_tail(&upper)?;
    for (marker, src) in [("CAM", Cam), ("TS", Telesync), ("TC", Telecine)] {
        if has_token(tail, marker) {
            return Some(src);
        }
    }
    None
}

/// Slice of `upper` after the first token that unambiguously belongs to
/// the release-attribute section (year, `SxxEyy`, language marker,
/// resolution). `None` when no such boundary exists — the whole string
/// might be title words, so ambiguous source tokens can't be trusted.
fn attribute_tail(upper: &str) -> Option<&str> {
    let bytes = upper.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        while start < bytes.len() && is_sep_byte(bytes[start]) {
            start += 1;
        }
        let mut end = start;
        while end < bytes.len() && !is_sep_byte(bytes[end]) {
            end += 1;
        }
        if end > start && is_attribute_boundary(&upper[start..end]) {
            return Some(&upper[end..]);
        }
        start = end;
    }
    None
}

fn is_attribute_boundary(token: &str) -> bool {
    // 4-digit year.
    if token.len() == 4
        && let Ok(y) = token.parse::<u32>()
        && (1900..=2099).contains(&y)
    {
        return true;
    }
    // SxxEyy / Sxx season-episode marker.
    if let Some(rest) = token.strip_prefix('S')
        && !rest.is_empty()
        && rest
            .split('E')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        return true;
    }
    matches!(
        token,
        // Language markers (same vocabulary as `detect_language`).
        "MULTI"
            | "FRENCH"
            | "TRUEFRENCH"
            | "SUBFRENCH"
            | "VOSTFR"
            | "VFF"
            | "VFQ"
            | "VFI"
            | "VF2"
            | "FR2"
            | "VOF"
            | "VOQ"
            | "VF"
            | "VO"
            | "ENGLISH"
            // Resolutions.
            | "2160P"
            | "1080P"
            | "720P"
            | "576P"
            | "480P"
            | "4K"
            | "UHD"
    )
}

/// Token-boundary contains: only matches `needle` when it sits
/// between separators (or at string ends). Without this `FRENCH`
/// would match against `FRENCHTOAST` and similar; matters less in
/// practice for these specific tokens but cheap to do right.
fn has_token(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let nbytes = needle.as_bytes();
    if nbytes.is_empty() || nbytes.len() > bytes.len() {
        return false;
    }
    let mut i = 0;
    while i + nbytes.len() <= bytes.len() {
        if &bytes[i..i + nbytes.len()] == nbytes {
            let before_ok = i == 0 || is_sep_byte(bytes[i - 1]);
            let after = i + nbytes.len();
            let after_ok = after == bytes.len() || is_sep_byte(bytes[after]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn is_sep_byte(b: u8) -> bool {
    matches!(b, b'.' | b'_' | b'-' | b' ' | b'[' | b']' | b'(' | b')')
}

/// Strip a trailing 4-digit year (1900-2099) from an already-
/// normalised title — `"lucky luke 1991"` → `"lucky luke"`. No-op
/// when no year is present. Used for TV identity so a show that
/// inlines its premiere year in SCENE filenames groups with a
/// follow whose user-facing title omits it.
pub fn strip_trailing_year(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() < 5 {
        return s.to_string();
    }
    let n = bytes.len();
    if bytes[n - 5] != b' ' {
        return s.to_string();
    }
    let tail = &bytes[n - 4..];
    if !tail.iter().all(u8::is_ascii_digit) {
        return s.to_string();
    }
    let year: u16 = std::str::from_utf8(tail).unwrap().parse().unwrap_or(0);
    if (1900..=2099).contains(&year) {
        s[..n - 5].to_string()
    } else {
        s.to_string()
    }
}

/// `Show.Name.S01E02.1080p...` → `"show name"` (lowercased, no
/// punctuation, single spaces).
pub fn normalize_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_space = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            for low in c.to_lowercase() {
                out.push(low);
            }
            last_was_space = false;
        } else if !last_was_space {
            out.push(' ');
            last_was_space = true;
        }
    }
    out.trim().to_string()
}

/// Parse a SCENE-style filename. Returns `None` only if there's nothing
/// recognisable — even a bare title produces a `Parsed { title, .. }`
/// with everything else null.
pub fn parse(filename: &str) -> Option<Parsed> {
    let stem = filename.rsplit_once('.').map_or(filename, |(s, _ext)| s);
    if stem.trim().is_empty() {
        return None;
    }

    // Find the structural boundary that separates the title from the
    // metadata tail. Priority: SXXEXX > year > quality > end-of-stem.
    let (title_end_byte, season, episode, year) = find_title_boundary(stem);
    // Leading "[GROUP]" is metadata, not title — it poisons the
    // collection key and the TMDB lookup. (The anime fallback below
    // strips its own bracketed shape.)
    let mut title = humanise(strip_leading_groups(&stem[..title_end_byte]));

    // Anime fallback (absolute numbering): only when this is neither a
    // SXXEXX TV release nor a year-tagged movie, and only for the
    // bracketed-group `[Group] Title - NN [tags]` shape. Strictly gated so
    // normal SCENE names never reach it and SXXEXX / year releases keep
    // their existing parse untouched.
    let mut absolute_episode = None;
    if season.is_none()
        && year.is_none()
        && let Some(am) = parse_anime(stem)
    {
        title = am.title;
        absolute_episode = am.episode;
    }

    if title.is_empty() {
        return None;
    }

    let quality = find_quality(stem);
    let source = find_source(stem);
    let group = find_group(stem);

    Some(Parsed {
        title,
        year,
        season,
        episode,
        quality,
        source,
        group,
        absolute_episode,
    })
}

/// `"[H4KIG] Ted 2"` → `"Ted 2"`. Falls back to the input when
/// stripping would leave nothing.
fn strip_leading_groups(s: &str) -> &str {
    let mut rest = s.trim_start();
    while let Some(tail) = rest.strip_prefix('[') {
        match tail.find(']') {
            Some(close) => rest = tail[close + 1..].trim_start_matches(['.', '_', ' ', '-']),
            None => break,
        }
    }
    if rest.trim().is_empty() { s } else { rest }
}

struct AnimeMatch {
    title: String,
    /// `None` for batch / season packs (`[Group] Title [tags]`, no per-episode
    /// number) — still a valid anime match, just without absolute numbering.
    episode: Option<u32>,
}

/// Parse the anime fansub shape `[Group] Title [- NN] [tags] [vN]` (absolute
/// episode numbering). Requires the leading `[group]` bracket; the ` - NN`
/// episode marker is OPTIONAL, because batch / season packs
/// (`[Delivroozzi] Sakamoto Desu ga [VOSTFR BD x265 1080p] v2`) are common and
/// must still yield a CLEAN title — otherwise the TMDB/AniList lookup keys on
/// `[Group] …` and fails. The title is the segment between the group bracket and
/// the first tag bracket (`[` / `(`), so leading group + trailing tags are both
/// stripped regardless of an episode marker.
fn parse_anime(stem: &str) -> Option<AnimeMatch> {
    let s = stem.trim();
    if !s.starts_with('[') {
        return None;
    }
    let close = s.find(']')?;
    let rest = s[close + 1..].trim();
    // Anime puts group, then title, then bracketed tags — so the title runs up
    // to the first tag bracket (or end-of-string for a bare name).
    let title_region = match rest.find(['[', '(']) {
        Some(i) => &rest[..i],
        None => rest,
    };
    // Optional trailing ` - NN` absolute episode inside the title region.
    let (title_str, episode) = match title_region.rfind(" - ") {
        Some(dash) => {
            let digits: String = title_region[dash + 3..]
                .trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            match digits.parse::<u32>() {
                Ok(ep) => (&title_region[..dash], Some(ep)),
                Err(_) => (title_region, None),
            }
        }
        None => (title_region, None),
    };
    let title = humanise(title_str.trim());
    if title.is_empty() {
        return None;
    }
    Some(AnimeMatch { title, episode })
}

/// Locate the byte index in `stem` where the title ends. Priority:
///   1. SXXEXX marker — TV release; everything before it is the title.
///   2. 4-digit year (1900–2099) at a word boundary — movie release.
///   3. Quality tag (1080p / 720p / 2160p) — fallback when no year.
///   4. End of stem — bare title with no metadata tags.
///
/// Returns `(title_end_byte, season, episode, year)`. `season`/`episode`
/// are populated only when the SE marker matched; `year` only when the
/// year boundary won (i.e., no SE marker, real year present).
fn find_title_boundary(stem: &str) -> (usize, Option<u32>, Option<u32>, Option<u16>) {
    if let Some(se) = find_se_marker(stem) {
        return (se.title_end, Some(se.season), Some(se.episode), None);
    }
    if let Some(yr) = find_year_boundary(stem) {
        return (yr.title_end, None, None, Some(yr.year));
    }
    if let Some(q_idx) = find_quality_index(stem) {
        return (
            stem[..q_idx].trim_end_matches(['.', '_', ' ', '-']).len(),
            None,
            None,
            None,
        );
    }
    (stem.len(), None, None, None)
}

struct SeMatch {
    title_end: usize,
    season: u32,
    episode: u32,
}

fn find_se_marker(stem: &str) -> Option<SeMatch> {
    // Two-pass: prefer a full `S01E02` marker (most precise), fall back
    // to a season-only `S01` marker (season packs / MULTI releases that
    // cover an entire season — `Show.Name.S01.MULTI.1080p…`). Without
    // the fallback, season packs got their title boundary placed at the
    // quality marker instead, leaking `S01 MULTI` into the parsed title
    // and breaking TMDB resolution for the parent collection.
    if let Some(m) = scan_for_se(stem, /* require_episode = */ true) {
        return Some(m);
    }
    scan_for_se(stem, /* require_episode = */ false)
}

fn scan_for_se(stem: &str, require_episode: bool) -> Option<SeMatch> {
    let bytes = stem.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Token boundary: only treat `S` as a marker when it stands at
        // the start of a fresh token (preceded by separator). Without
        // this, `S` inside a word like "Squid" or "Lassie" matches.
        let at_boundary = i == 0 || matches!(bytes[i - 1], b'.' | b'_' | b' ' | b'-' | b'(' | b'[');
        if at_boundary && (bytes[i] == b'S' || bytes[i] == b's') {
            let mut j = i + 1;
            let mut s_digits = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() && s_digits < 4 {
                j += 1;
                s_digits += 1;
            }
            if s_digits > 0 {
                let s_end = j;
                // Allow an optional separator run between the season and
                // the episode marker. `S02E02`, `S02 E02`, `S02.E02` and
                // `S02 - E02` all denote S02E02 — some packs name their
                // leaves `Show - S02 E02.mkv`. Cap the run at three chars
                // so we skip ` - ` but never leap a whole token (so
                // `S02 1080p` still falls through to the season-pack
                // path with episode 0 rather than mis-reading `1080p`).
                let mut e_pos = j;
                let mut gap = 0;
                while e_pos < bytes.len()
                    && gap < 3
                    && matches!(bytes[e_pos], b'.' | b'_' | b' ' | b'-')
                {
                    e_pos += 1;
                    gap += 1;
                }
                let has_e = e_pos < bytes.len() && (bytes[e_pos] == b'E' || bytes[e_pos] == b'e');
                if has_e {
                    let mut k = e_pos + 1;
                    let mut e_digits = 0;
                    while k < bytes.len() && bytes[k].is_ascii_digit() && e_digits < 4 {
                        k += 1;
                        e_digits += 1;
                    }
                    if e_digits > 0
                        && let (Ok(s), Ok(e)) = (
                            stem[i + 1..s_end].parse::<u32>(),
                            stem[e_pos + 1..k].parse::<u32>(),
                        )
                    {
                        let title_end = stem[..i].trim_end_matches(['.', '_', ' ', '-']).len();
                        return Some(SeMatch {
                            title_end,
                            season: s,
                            episode: e,
                        });
                    }
                }
                // Season-only fallback (`S01` with no `E\d+`). Only
                // accept it when (a) the caller asked for the relaxed
                // form and (b) the next char is a real separator, so
                // we don't false-match a chunk like `S01ABC` that
                // isn't actually a season marker.
                if !require_episode {
                    let next_ok = j == bytes.len()
                        || matches!(bytes[j], b'.' | b'_' | b' ' | b'-' | b']' | b')');
                    if next_ok && let Ok(s) = stem[i + 1..s_end].parse::<u32>() {
                        let title_end = stem[..i].trim_end_matches(['.', '_', ' ', '-']).len();
                        return Some(SeMatch {
                            title_end,
                            season: s,
                            // `episode = 0` is the in-band sentinel
                            // for "season pack" — `Parsed::is_tv()`
                            // only checks `season.is_some()`, so
                            // downstream classification still works,
                            // and SCENE-grouping uses the
                            // (collection_id, season, episode) key
                            // for episode-level joins which a
                            // season pack shouldn't appear in.
                            episode: 0,
                        });
                    }
                }
            }
        }
        i += 1;
    }
    None
}

struct YearMatch {
    title_end: usize,
    year: u16,
}

fn find_year_boundary(stem: &str) -> Option<YearMatch> {
    let bytes = stem.as_bytes();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if bytes[i..i + 4].iter().all(u8::is_ascii_digit) {
            let prev_ok = i == 0 || !bytes[i - 1].is_ascii_digit();
            let next_ok = i + 4 == bytes.len() || !bytes[i + 4].is_ascii_digit();
            if prev_ok
                && next_ok
                && let Ok(y) = std::str::from_utf8(&bytes[i..i + 4])
                    .unwrap()
                    .parse::<u16>()
                && (1900..=2099).contains(&y)
            {
                // Skip leading-position years (e.g., "2024.Show.Name" —
                // unlikely, but the year shouldn't eat the title).
                if i > 0 {
                    let title_end = stem[..i].trim_end_matches(['.', '_', ' ', '-', '(']).len();
                    return Some(YearMatch { title_end, year: y });
                }
            }
        }
        i += 1;
    }
    None
}

fn find_quality_index(stem: &str) -> Option<usize> {
    for q in ["2160p", "1080p", "720p", "480p"] {
        if let Some(idx) = stem.find(q) {
            return Some(idx);
        }
    }
    None
}

/// Replace `.`/`_` with spaces, collapse runs of whitespace, trim.
fn humanise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for c in s.chars() {
        let is_sep = matches!(c, '.' | '_' | '\t');
        let is_space = c.is_whitespace() || is_sep;
        if is_space {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim().to_string()
}

fn find_quality(s: &str) -> Option<String> {
    for q in ["2160p", "1080p", "720p", "480p"] {
        if s.contains(q) {
            return Some(q.to_string());
        }
    }
    None
}

fn find_source(s: &str) -> Option<String> {
    // Order matters — match longer / more specific tags first so
    // "WEB-DL" doesn't get caught by a "WEB" check.
    for src in [
        "WEB-DL", "WEBRip", "BluRay", "BDRip", "HDTV", "DVDRip", "WEB",
    ] {
        if s.contains(src) {
            return Some(src.to_string());
        }
    }
    None
}

fn find_group(stem: &str) -> Option<String> {
    // Convention: trailing `-GROUP` after the last `-`, group is alnum.
    // E.g., `Show.S01E02.1080p.WEB-DL.x264-BULiTT` → `BULiTT`.
    let last = stem.rsplit('-').next()?;
    if last.is_empty() || last.len() > 30 {
        return None;
    }
    if last.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(last.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tv_release() {
        let p = parse("Squid.Game.S02E03.1080p.NF.WEB-DL.DDP5.1.x264-BULiTT.mkv").unwrap();
        assert_eq!(p.title, "Squid Game");
        assert_eq!(p.season, Some(2));
        assert_eq!(p.episode, Some(3));
        assert_eq!(p.quality.as_deref(), Some("1080p"));
        assert_eq!(p.source.as_deref(), Some("WEB-DL"));
        assert_eq!(p.group.as_deref(), Some("BULiTT"));
        assert_eq!(p.year, None);
        assert!(p.is_tv());
    }

    #[test]
    fn parses_movie_release() {
        let p = parse("My.Dearest.Assassin.2026.MULTi.1080p.WEB.H265-BULiTT.mkv").unwrap();
        assert_eq!(p.title, "My Dearest Assassin");
        assert_eq!(p.year, Some(2026));
        assert_eq!(p.season, None);
        assert_eq!(p.episode, None);
        assert_eq!(p.quality.as_deref(), Some("1080p"));
        assert_eq!(p.source.as_deref(), Some("WEB"));
        assert_eq!(p.group.as_deref(), Some("BULiTT"));
        assert!(p.is_movie());
    }

    #[test]
    fn parses_anime_absolute_episode() {
        let p = parse("[SubsPlease] Frieren - 12 [1080p][HEVC].mkv").unwrap();
        assert_eq!(p.title, "Frieren");
        assert_eq!(p.absolute_episode, Some(12));
        assert_eq!(p.season, None);
        assert_eq!(p.episode, None);
        assert_eq!(p.year, None);
    }

    #[test]
    fn parses_anime_batch_pack_without_episode() {
        // A batch / season pack has no ` - NN` episode marker. The title must
        // still drop the leading fansub group ([Delivroozzi]) and the trailing
        // tag bracket — otherwise the TMDB/AniList lookup keys on the group and
        // finds nothing (the prod regression).
        let p = parse("[Delivroozzi] Sakamoto Desu ga [VOSTFR BD x265 10bits 1080p FLAC] v2.mkv")
            .unwrap();
        assert_eq!(p.title, "Sakamoto Desu ga");
        assert_eq!(p.absolute_episode, None);
        assert_eq!(p.season, None);
        assert_eq!(p.episode, None);
    }

    #[test]
    fn anime_handles_version_and_long_episode() {
        let v = parse("[Group] Mob Psycho 100 - 05v2 [720p].mkv").unwrap();
        assert_eq!(v.title, "Mob Psycho 100");
        assert_eq!(v.absolute_episode, Some(5));

        let long = parse("[Erai-raws] One Piece - 1080 [1080p].mkv").unwrap();
        assert_eq!(long.title, "One Piece");
        assert_eq!(long.absolute_episode, Some(1080));
    }

    #[test]
    fn anime_branch_is_strictly_gated() {
        // SXXEXX TV release: anime branch must not fire.
        let tv = parse("Squid.Game.S02E03.1080p.NF.WEB-DL.x264-BULiTT.mkv").unwrap();
        assert_eq!(tv.absolute_episode, None);
        assert_eq!(tv.season, Some(2));

        // Year-tagged movie: anime branch must not fire.
        let movie = parse("Dune.Part.Two.2024.1080p.WEB.H265-GRP.mkv").unwrap();
        assert_eq!(movie.absolute_episode, None);
        assert_eq!(movie.year, Some(2024));

        // Bracketed but year-tagged → year wins, no absolute episode.
        let bracket_year = parse("[Grp] Some Movie 2020 1080p.mkv").unwrap();
        assert_eq!(bracket_year.absolute_episode, None);

        // No leading group bracket → not treated as anime.
        let dashed = parse("Some Show - Special.mkv").unwrap();
        assert_eq!(dashed.absolute_episode, None);
    }

    #[test]
    fn handles_short_marker() {
        // Some releases skip the leading zero — `S1E2` instead of `S01E02`.
        let p = parse("Show.Name.S1E2.720p.HDTV.mkv").unwrap();
        assert_eq!(p.title, "Show Name");
        assert_eq!(p.season, Some(1));
        assert_eq!(p.episode, Some(2));
    }

    #[test]
    fn handles_spaced_se_marker() {
        // Real-world season pack whose leaves separate the season and
        // episode tokens with a space (and a dashed title prefix):
        // `The Promised Neverland - S02 E02.mkv`. Before the gap-skip
        // this fell through to the season-pack path and every leaf got
        // stamped episode 0, so the Collections UI showed S02E00 for
        // the whole season.
        let p = parse("The Promised Neverland - S02 E02.mkv").unwrap();
        assert_eq!(p.title, "The Promised Neverland");
        assert_eq!(p.season, Some(2));
        assert_eq!(p.episode, Some(2));
        assert!(p.is_tv());

        // Dotted and dashed gaps resolve the same way.
        let dot = parse("Show.Name.S03.E07.1080p.WEB-DL-X.mkv").unwrap();
        assert_eq!((dot.season, dot.episode), (Some(3), Some(7)));
        let dash = parse("Show Name - S03 - E07.mkv").unwrap();
        assert_eq!((dash.season, dash.episode), (Some(3), Some(7)));
    }

    #[test]
    fn spaced_season_then_token_stays_a_pack() {
        // Guard the gap-skip: a separator after the season followed by a
        // non-`E` token must NOT be read as an episode — it stays a
        // season pack (episode 0), exactly as before the fix.
        let p = parse("Silicon Valley S01 1080p BluRay x264-XYZ.mkv").unwrap();
        assert_eq!(p.season, Some(1));
        assert_eq!(p.episode, Some(0)); // sentinel for season-pack
        assert!(p.is_tv());
    }

    #[test]
    fn parses_season_pack_release() {
        // Season-only marker, no `EXX` — typical of MULTI / COMPLETE
        // packs that group every episode into one torrent. Without
        // this case, the title boundary fell back to the quality
        // marker and `S01.MULTI` leaked into the parsed title.
        let p = parse("Silicon.Valley.S01.MULTi.1080p.BluRay.x264-XYZ.mkv").unwrap();
        assert_eq!(p.title, "Silicon Valley");
        assert_eq!(p.season, Some(1));
        assert_eq!(p.episode, Some(0)); // sentinel for season-pack
        assert!(p.is_tv());
        // The dedup key the collection uses to group siblings is just
        // the title — no episode marker leaks into it.
        assert_eq!(p.normalized_key(), "silicon valley");
    }

    #[test]
    fn parses_season_pack_no_metadata() {
        // Bare `S01` at the end of the title with no quality / source
        // tail. Still split the season off so the cleaned title is
        // usable for TMDB resolution.
        let p = parse("Some.Show.S03.mkv").unwrap();
        assert_eq!(p.title, "Some Show");
        assert_eq!(p.season, Some(3));
        assert!(p.is_tv());
    }

    #[test]
    fn normalised_key_dedups_release_variants() {
        let a = parse("Squid.Game.S02E03.1080p.NF.WEB-DL.x264-BULiTT.mkv").unwrap();
        let b = parse("Squid_Game_S02E04_2160p_NF_WEB-DL_x265-OTHER.mkv").unwrap();
        assert_eq!(a.normalized_key(), b.normalized_key());
        assert_eq!(a.normalized_key(), "squid game");
    }

    #[test]
    fn returns_none_on_empty_filename() {
        assert!(parse("").is_none());
        assert!(parse(".mkv").is_none());
    }

    #[test]
    fn collection_key_includes_year_for_movies() {
        let dune84 = parse("Dune.1984.1080p.BluRay.x264-XYZ.mkv").unwrap();
        let dune21 = parse("Dune.2021.2160p.WEB-DL.x265-ABC.mkv").unwrap();
        assert_ne!(dune84.collection_key(false), dune21.collection_key(false));
        assert_eq!(dune84.collection_key(false), "dune 1984");
        assert_eq!(dune21.collection_key(false), "dune 2021");
    }

    #[test]
    fn collection_key_drops_year_for_tv() {
        // Same show, different episodes — should land in one bucket
        // regardless of any year noise in the filename.
        let s1 = parse("Squid.Game.S01E02.1080p.NF.WEB-DL-X.mkv").unwrap();
        let s2 = parse("Squid.Game.S02E03.1080p.NF.WEB-DL-Y.mkv").unwrap();
        assert_eq!(s1.collection_key(true), s2.collection_key(true));
        assert_eq!(s1.collection_key(true), "squid game");
    }

    #[test]
    fn collection_key_strips_inlined_year_on_tv() {
        // Real-world case: SCENE filename inlines the show's
        // premiere year before the SE marker. The user's follow
        // is just "Lucky Luke" → key "lucky luke". The torrent
        // SCENE name "Lucky.Luke.1991.S01E01" parses to title
        // "Lucky Luke 1991" → key MUST collapse to "lucky luke"
        // for the join to match.
        let p = parse("Lucky.Luke.1991.S01E01.FRENCH.HDTV.x264-XYZ.mkv").unwrap();
        assert_eq!(p.collection_key(true), "lucky luke");
    }

    #[test]
    fn series_key_aligns_user_input_and_scene() {
        // Public helper used by follows.rs::create. A user-typed
        // "Lucky Luke" must canonicalise to the same string the
        // SCENE-derived collection key produces.
        assert_eq!(series_key("Lucky Luke"), "lucky luke");
        assert_eq!(series_key("Lucky.Luke.1991"), "lucky luke");
        assert_eq!(series_key("LUCKY  LUKE  1991"), "lucky luke");
        assert_eq!(
            series_key("Lucky Luke 1991"),
            parse("Lucky.Luke.1991.S01E01.FRENCH.HDTV.x264-XYZ.mkv")
                .unwrap()
                .collection_key(true),
        );
    }

    #[test]
    fn strip_trailing_year_handles_edges() {
        assert_eq!(strip_trailing_year("squid game"), "squid game");
        assert_eq!(strip_trailing_year("squid game 2021"), "squid game");
        // Out-of-range "year" — left in place.
        assert_eq!(strip_trailing_year("squid game 1234"), "squid game 1234");
        assert_eq!(strip_trailing_year("show 2099"), "show");
        assert_eq!(strip_trailing_year("show 2100"), "show 2100");
        // No leading space — not a year suffix.
        assert_eq!(strip_trailing_year("show2024"), "show2024");
        assert_eq!(strip_trailing_year(""), "");
        assert_eq!(strip_trailing_year("a"), "a");
    }

    #[test]
    fn display_with_year_movie_format() {
        let p = parse("My.Dearest.Assassin.2026.MULTi.1080p.WEB.H265-X.mkv").unwrap();
        assert_eq!(p.display_with_year(false), "My Dearest Assassin (2026)");
        // TV ignores year in the display.
        let tv = parse("Squid.Game.S02E03.1080p.NF.WEB-DL-X.mkv").unwrap();
        assert_eq!(tv.display_with_year(true), "Squid Game");
    }

    #[test]
    fn detect_language_real_world_titles() {
        // The exact strings shipping from indexers today — captured
        // from a live `/api/search?q=classroom%20of%20the%20elite`
        // response so this stays grounded in reality, not in what we
        // think tracker naming conventions should be.
        assert_eq!(
            detect_language(
                "Classroom.Of.The.Elite.S04E11.VOSTFR.1080p.WEB.AAC.2.0.x264-Tsundere-Raws"
            ),
            Language::French,
        );
        assert_eq!(
            detect_language(
                "Classroom.of.the.Elite.S04E11.SUBFRENCH.1080p.CR.WEB.x264.AAC-Tsundere-Raws"
            ),
            Language::French,
        );
        assert_eq!(
            detect_language(
                "Classroom.of.the.Elite.S04E06.MULTi.AD.1080p.CR.WEB-DL.AAC2.0.x264-Tsundere-Raws"
            ),
            Language::Multi,
        );
        // Seedpool — no language tag at all. Treated as Unknown so
        // the matcher will refuse a French-preferring collection
        // but accept an English one.
        assert_eq!(
            detect_language("Classroom.of.the.Elite.S04E11.1080p.CR.WEB-DL.AAC2.0.H.264-VARYG"),
            Language::Unknown,
        );
        assert_eq!(
            detect_language("Show.Name.S01E01.VFF.1080p.BluRay.x264-XYZ"),
            Language::French,
        );
        assert_eq!(
            detect_language("Show.Name.S01E01.TRUEFRENCH.1080p.BluRay.x264-XYZ"),
            Language::French,
        );
        // VF2 / FR2 — both French dubs (VFF + VFQ) present. The
        // user flagged this convention after seeing it on real
        // c411 / TOS releases. Still resolves to French (no
        // dedicated variant — VF2 is a "more complete French"
        // signal, not a distinct language).
        assert_eq!(
            detect_language("Show.Name.S01E01.VF2.1080p.WEB.x264-XYZ"),
            Language::French,
        );
        assert_eq!(
            detect_language("Show.Name.S01E01.FR2.1080p.WEB.x264-XYZ"),
            Language::French,
        );
    }

    #[test]
    fn language_satisfies_matrix() {
        // French collection: accept FR + Multi; reject EN + Unknown.
        assert!(Language::French.satisfies(Language::French));
        assert!(Language::Multi.satisfies(Language::French));
        assert!(!Language::English.satisfies(Language::French));
        assert!(!Language::Unknown.satisfies(Language::French));

        // English collection: accept EN + Multi + Unknown; reject FR.
        assert!(Language::English.satisfies(Language::English));
        assert!(Language::Multi.satisfies(Language::English));
        assert!(Language::Unknown.satisfies(Language::English));
        assert!(!Language::French.satisfies(Language::English));

        // Unknown preference (first ingest, nothing established): take
        // anything.
        assert!(Language::French.satisfies(Language::Unknown));
        assert!(Language::English.satisfies(Language::Unknown));
        assert!(Language::Multi.satisfies(Language::Unknown));
        assert!(Language::Unknown.satisfies(Language::Unknown));
    }

    #[test]
    fn language_token_boundaries_prevent_substring_false_positives() {
        // `FRENCH` inside another word doesn't trigger French
        // detection. The token-boundary check covers this even
        // though these specific collisions are rare.
        assert_eq!(
            detect_language("FRENCHTOAST.S01E01.1080p.WEB.H264"),
            Language::Unknown,
        );
        // Genuine separator → does match.
        assert_eq!(
            detect_language("FRENCHTOAST.S01E01.FRENCH.1080p.WEB.H264"),
            Language::French,
        );
    }

    #[test]
    fn detect_codec_real_world_titles() {
        assert_eq!(
            detect_codec("Squid.Game.S02E03.1080p.NF.WEB-DL.x264-BULiTT.mkv"),
            Codec::H264,
        );
        assert_eq!(
            detect_codec("My.Dearest.Assassin.2026.MULTi.1080p.WEB.H265-BULiTT.mkv"),
            Codec::Hevc,
        );
        assert_eq!(
            detect_codec("[SubsPlease] Frieren - 12 [1080p][HEVC].mkv"),
            Codec::Hevc,
        );
        assert_eq!(
            detect_codec(
                "[Delivroozzi] Sakamoto Desu ga [VOSTFR BD x265 10bits 1080p FLAC] v2.mkv"
            ),
            Codec::Hevc,
        );
        // Dotted `H.264` — a real Seedpool naming convention, also
        // exercised by `detect_language_real_world_titles` above.
        assert_eq!(
            detect_codec("Classroom.of.the.Elite.S04E11.1080p.CR.WEB-DL.AAC2.0.H.264-VARYG"),
            Codec::H264,
        );
        assert_eq!(
            detect_codec("Show.Name.S01E01.1080p.WEB.AV1-XYZ"),
            Codec::Av1,
        );
        assert_eq!(
            detect_codec("Show.Name.S01E01.1080p.WEB.VP9-XYZ"),
            Codec::Vp9,
        );
        // No codec tag at all — common, never claim a codec without
        // evidence.
        assert_eq!(
            detect_codec("Show.Name.S01E01.1080p.WEB-DL.AAC2.0-GROUP"),
            Codec::Unknown,
        );
    }

    #[test]
    fn dubious_source_real_world_titles() {
        // Fresh theatrical FR releases — the exact shape users get
        // fooled by (TS/CAM + MD micro-dub after the year marker).
        assert_eq!(
            detect_dubious_source("Vice.Versa.2.2024.FRENCH.TS.MD.720p.x264-GROUP"),
            Some(DubiousSource::Telesync),
        );
        assert_eq!(
            detect_dubious_source("Un.Film.2026.FRENCH.CAM.MD.1080p.H264"),
            Some(DubiousSource::Cam),
        );
        // Unambiguous markers need no attribute boundary.
        assert_eq!(
            detect_dubious_source("Superman.2025.HDCAM.c1nem4.x264"),
            Some(DubiousSource::Cam),
        );
        assert_eq!(
            detect_dubious_source("Movie.Name.TELESYNC.VOSTFR"),
            Some(DubiousSource::Telesync),
        );
        assert_eq!(
            detect_dubious_source("Film.2025.HDTC.1080p.x265"),
            Some(DubiousSource::Telecine),
        );
        assert_eq!(
            detect_dubious_source("Oscar.Bait.2026.DVDScr.XviD"),
            Some(DubiousSource::Screener),
        );
        // Language marker alone is a valid boundary (no year present).
        assert_eq!(
            detect_dubious_source("Deadpool.Wolverine.FRENCH.TS.LD"),
            Some(DubiousSource::Telesync),
        );
        // "Cam" as a title word must NOT trigger — 2018 film.
        assert_eq!(detect_dubious_source("Cam.2018.1080p.WEB-DL.x264"), None);
        // Ambiguous tokens before any boundary stay inert.
        assert_eq!(detect_dubious_source("Counter.Strike.TS.Documentary"), None);
        // Clean releases.
        assert_eq!(
            detect_dubious_source("Dune.Part.Two.2024.2160p.WEB-DL.HEVC-FLUX"),
            None,
        );
        assert_eq!(
            detect_dubious_source(
                "Classroom.of.the.Elite.S04E11.1080p.CR.WEB-DL.AAC2.0.H.264-VARYG"
            ),
            None,
        );
    }

    #[test]
    fn compare_video_files_sorts_tv_pack_by_episode() {
        // Real-world Stranger Things season pack from the user.
        // Without SCENE-aware ordering these come out by size
        // (2.0 → 1.9 → 1.8 → ...), interleaving episodes randomly.
        let mut files = [
            "Stranger Things S02E03 MULTi VFI 4KLight HDR BluRay AC3 5.1 x265-QTZ.mkv",
            "Stranger Things S02E02 MULTi VFI 4KLight HDR BluRay AC3 5.1 x265-QTZ.mkv",
            "Stranger Things S02E01 MULTi VFI 4KLight HDR BluRay AC3 5.1 x265-QTZ.mkv",
            "Stranger Things S02E04 MULTi VFI 4KLight HDR BluRay AC3 5.1 x265-QTZ.mkv",
            "Stranger Things S02E09 MULTi VFI 4KLight HDR BluRay AC3 5.1 x265-QTZ.mkv",
            "Stranger Things S02E05 MULTi VFI 4KLight HDR BluRay AC3 5.1 x265-QTZ.mkv",
        ];
        files.sort_by(|a, b| compare_video_files(a, b));
        let eps: Vec<&str> = files
            .iter()
            .filter_map(|f| f.split_whitespace().nth(2))
            .collect();
        assert_eq!(
            eps,
            ["S02E01", "S02E02", "S02E03", "S02E04", "S02E05", "S02E09"]
        );
    }

    #[test]
    fn compare_video_files_se_marked_before_extras() {
        // Featurettes / extras drop to the end. Common pattern:
        // a season pack ships the episodes + a "Behind the Scenes"
        // file with no SE marker.
        let mut files = vec![
            "Behind.The.Scenes.mkv",
            "Show.Name.S01E02.mkv",
            "Bloopers.mkv",
            "Show.Name.S01E01.mkv",
        ];
        files.sort_by(|a, b| compare_video_files(a, b));
        assert_eq!(
            files,
            vec![
                "Show.Name.S01E01.mkv",
                "Show.Name.S01E02.mkv",
                "Behind.The.Scenes.mkv",
                "Bloopers.mkv",
            ],
        );
    }

    #[test]
    fn compare_video_files_uses_basename_only() {
        // Multi-disc packs (Disc1/EpisodeXX.mkv, Disc2/EpisodeYY.mkv)
        // sort by the SCENE marker, not by directory prefix.
        let mut files = vec![
            "Disc2/Show.S01E04.mkv",
            "Disc1/Show.S01E01.mkv",
            "Disc2/Show.S01E03.mkv",
            "Disc1/Show.S01E02.mkv",
        ];
        files.sort_by(|a, b| compare_video_files(a, b));
        assert_eq!(
            files,
            vec![
                "Disc1/Show.S01E01.mkv",
                "Disc1/Show.S01E02.mkv",
                "Disc2/Show.S01E03.mkv",
                "Disc2/Show.S01E04.mkv",
            ],
        );
    }

    #[test]
    fn handles_no_se_marker_as_movie() {
        // A bare title without SXXEXX or year shouldn't crash; we just
        // get a Parsed with mostly nulls.
        let p = parse("Some.Random.File.mkv").unwrap();
        assert_eq!(p.title, "Some Random File");
        assert_eq!(p.season, None);
        assert_eq!(p.year, None);
        assert!(p.is_movie());
    }

    #[test]
    fn fleuve_anime_classifies_and_keys_apart() {
        // The reported bug: a fansub fleuve release with the absolute
        // episode crammed into a fake S01.
        let name = "One Piece S01E1156 VOSTFR 1080p WEB x264 AAC -Tsundere-Raws (CR).mkv";
        let p = parse(name).unwrap();
        assert_eq!(p.title, "One Piece");
        assert_eq!(p.season, Some(1));
        assert_eq!(p.episode, Some(1156));

        assert!(looks_like_anime_release(name, p.season, p.episode));
        assert_eq!(absolute_from_parsed(&p), Some(1156));

        // Anime and live-action with the same title must key apart.
        let anime_key = p.collection_key_kind(true, true);
        let live_action_key = p.collection_key_kind(true, false);
        assert_eq!(anime_key, "anime:one piece");
        assert_eq!(live_action_key, "one piece");
        assert_ne!(anime_key, live_action_key);
    }

    #[test]
    fn season_zero_oav_special_classifies_as_anime() {
        // The reported bug: a season-0 OAV special from a plain scene
        // group (no bracket fansub prefix, no known fansub-group token)
        // used to slip through unclassified because the fleuve check is
        // gated on `season == Some(1)`, which never matches season 0 —
        // silently splitting the special into its own orphan collection
        // instead of joining the main (anime-classified) series.
        let name = "Another.S00E01.(OAV).VOSTFR.1080p.10bits.BluRay.x265.AAC-Punisher694.mkv";
        let p = parse(name).unwrap();
        assert_eq!(p.title, "Another");
        assert_eq!(p.season, Some(0));
        assert_eq!(p.episode, Some(1));
        assert!(looks_like_anime_release(name, p.season, p.episode));
        // Must key the same way as the main S01 episodes so the special
        // joins the existing anime collection instead of forking one.
        assert_eq!(p.collection_key_kind(true, true), "anime:another");
    }

    #[test]
    fn seasonal_anime_stays_seasonal() {
        // A modern season-cut anime: classified anime (group token) but
        // NOT absolute-numbered — it must keep ordinary seasonal display.
        let name = "[SubsPlease] Demon Slayer S02E05 1080p.mkv";
        let p = parse(name).unwrap();
        assert!(looks_like_anime_release(name, p.season, p.episode));
        assert_eq!(absolute_from_parsed(&p), None);
    }

    #[test]
    fn non_anime_is_not_flagged() {
        let name = "Squid.Game.S02E03.1080p.NF.WEB-DL.x264-BULiTT.mkv";
        let p = parse(name).unwrap();
        assert!(!looks_like_anime_release(name, p.season, p.episode));
        assert_eq!(absolute_from_parsed(&p), None);
        // Non-anime never gets the prefix even if is_anime is wrongly true
        // for a non-TV call.
        assert_eq!(p.collection_key_kind(true, false), "squid game");
    }

    #[test]
    fn high_episode_count_alone_does_not_flag_anime() {
        // A daily/long-running NON-anime show with absolute-ish numbering
        // and no anime naming signal must NOT be classified anime.
        let name = "Some.Daily.Show.S01E812.1080p.WEB.x264-GRP.mkv";
        let p = parse(name).unwrap();
        assert!(!looks_like_anime_release(name, p.season, p.episode));
    }

    #[test]
    fn leading_bracket_group_is_stripped_from_titles() {
        // Regression (prod "[H4KIG] Ted 2 (2015)" collection): a leading
        // "[GROUP]" survives the year/SE boundary because only the anime
        // fallback (gated on no-year, no-SE) knew how to strip brackets.
        // It must never reach `display_title` — it poisons the collection
        // key and the TMDB lookup, which breaks the poster.
        let movie = parse("[H4KIG] Ted 2 (2015) MULTI VFF 1080p BluRay x265-H4KIG.mkv").unwrap();
        assert_eq!(movie.title, "Ted 2");
        assert_eq!(movie.year, Some(2015));
        assert_eq!(movie.display_with_year(false), "Ted 2 (2015)");

        let episode = parse("[H4KIG] The Rig S02E03 MULTI 1080p WEB x265.mkv").unwrap();
        assert_eq!(episode.title, "The Rig");
        assert_eq!((episode.season, episode.episode), (Some(2), Some(3)));

        // The anime shape keeps working through its dedicated path.
        let anime = parse("[SubsPlease] Sousou no Frieren - 05 (1080p).mkv").unwrap();
        assert_eq!(anime.title, "Sousou no Frieren");
        assert_eq!(anime.absolute_episode, Some(5));

        // A stem that is ONLY a bracket group keeps its verbatim shape
        // rather than parsing to an empty title.
        assert!(parse("[weird-name].mkv").is_some());
    }

    #[test]
    fn display_title_carries_year_for_tmdb_resolution() {
        // Regression (prod "Supernatural (2005)" → "Pimp my ride" poster):
        // collection TMDB resolution must run on the SCENE *identity*
        // (`display_title`), not the torrent name. c411 names season
        // packs "Saison N" (French "Season N") — a useless query that
        // returned an unrelated French show as the top TV hit. The
        // display_title is built from the Plex/Sonarr-style file leaf,
        // and re-parsing it must recover a clean title + the year hint
        // that disambiguates same-title shows.
        let leaf = parse(
            "Supernatural (2005) - S01E01 - Pilot - 2005-09-13 - Audio [FR+EN] - SubTitle [FR+EN] - Quality [WEBDL-1080p].mkv",
        )
        .unwrap();
        // The file leaf parses to a TV identity; the inline "(2005)"
        // stays in the title (SE marker wins the boundary).
        assert_eq!(leaf.season, Some(1));
        assert_eq!(leaf.episode, Some(1));
        assert_eq!(leaf.title, "Supernatural (2005)");
        let display = leaf.display_with_year(true);
        assert_eq!(display, "Supernatural (2005)");

        // Re-parsing the display_title (no SE marker) lets the year
        // boundary fire: clean title + year hint for `pick_best`.
        let from_display = parse(&display).unwrap();
        assert_eq!(from_display.title, "Supernatural");
        assert_eq!(from_display.year, Some(2005));
        assert_eq!(series_key(&from_display.title), "supernatural");

        // The torrent name, by contrast, is unusable: no year hint and a
        // garbage title that would feed `multi_search` the wrong query.
        let torrent_name = parse("Saison 1").unwrap();
        assert_eq!(torrent_name.year, None);
        assert_eq!(torrent_name.title, "Saison 1");
    }
}
