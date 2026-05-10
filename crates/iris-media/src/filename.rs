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
    let stem = filename
        .rsplit_once('.')
        .map_or(filename, |(s, _ext)| s);
    if stem.trim().is_empty() {
        return None;
    }

    // Find the structural boundary that separates the title from the
    // metadata tail. Priority: SXXEXX > year > quality > end-of-stem.
    let (title_end_byte, season, episode, year) = find_title_boundary(stem);
    let title_part = &stem[..title_end_byte];
    let title = humanise(title_part);
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
    })
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
            stem[..q_idx]
                .trim_end_matches(['.', '_', ' ', '-'])
                .len(),
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
        let at_boundary = i == 0
            || matches!(bytes[i - 1], b'.' | b'_' | b' ' | b'-' | b'(' | b'[');
        if at_boundary && (bytes[i] == b'S' || bytes[i] == b's') {
            let mut j = i + 1;
            let mut s_digits = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() && s_digits < 4 {
                j += 1;
                s_digits += 1;
            }
            if s_digits > 0 {
                let s_end = j;
                let has_e = j < bytes.len() && (bytes[j] == b'E' || bytes[j] == b'e');
                if has_e {
                    let mut k = j + 1;
                    let mut e_digits = 0;
                    while k < bytes.len() && bytes[k].is_ascii_digit() && e_digits < 4 {
                        k += 1;
                        e_digits += 1;
                    }
                    if e_digits > 0 {
                        if let (Ok(s), Ok(e)) = (
                            stem[i + 1..s_end].parse::<u32>(),
                            stem[j + 1..k].parse::<u32>(),
                        ) {
                            let title_end = stem[..i]
                                .trim_end_matches(['.', '_', ' ', '-'])
                                .len();
                            return Some(SeMatch {
                                title_end,
                                season: s,
                                episode: e,
                            });
                        }
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
                    if next_ok {
                        if let Ok(s) = stem[i + 1..s_end].parse::<u32>() {
                            let title_end = stem[..i]
                                .trim_end_matches(['.', '_', ' ', '-'])
                                .len();
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
            if prev_ok && next_ok {
                if let Ok(y) = std::str::from_utf8(&bytes[i..i + 4]).unwrap().parse::<u16>() {
                    if (1900..=2099).contains(&y) {
                        // Skip leading-position years (e.g., "2024.Show.Name" —
                        // unlikely, but the year shouldn't eat the title).
                        if i > 0 {
                            let title_end = stem[..i]
                                .trim_end_matches(['.', '_', ' ', '-', '('])
                                .len();
                            return Some(YearMatch {
                                title_end,
                                year: y,
                            });
                        }
                    }
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
    for src in ["WEB-DL", "WEBRip", "BluRay", "BDRip", "HDTV", "DVDRip", "WEB"] {
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
    fn handles_short_marker() {
        // Some releases skip the leading zero — `S1E2` instead of `S01E02`.
        let p = parse("Show.Name.S1E2.720p.HDTV.mkv").unwrap();
        assert_eq!(p.title, "Show Name");
        assert_eq!(p.season, Some(1));
        assert_eq!(p.episode, Some(2));
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
    fn handles_no_se_marker_as_movie() {
        // A bare title without SXXEXX or year shouldn't crash; we just
        // get a Parsed with mostly nulls.
        let p = parse("Some.Random.File.mkv").unwrap();
        assert_eq!(p.title, "Some Random File");
        assert_eq!(p.season, None);
        assert_eq!(p.year, None);
        assert!(p.is_movie());
    }
}
