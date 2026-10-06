//! XMLTV programme guide for Live TV's now/next overlay.
//!
//! The source (e.g. xmltvfr.fr) publishes a gzipped XMLTV document refreshed
//! daily. We gunzip explicitly (it is a gzipped *body* served as
//! `application/x-gzip`, not transport encoding — reqwest's gzip feature
//! never sees it), stream-parse with quick-xml, and keep a bounded window of
//! programmes per channel in memory.

use std::collections::HashMap;
use std::io::Read;

use chrono::{DateTime, FixedOffset, NaiveDateTime, Utc};
use quick_xml::Reader;
use quick_xml::events::{BytesRef, Event};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Programme {
    pub start: DateTime<Utc>,
    pub stop: DateTime<Utc>,
    pub title: String,
    pub category: Option<String>,
    pub description: Option<String>,
}

/// Programmes indexed by lowercase XMLTV channel id, sorted by start time.
#[derive(Debug, Default)]
pub struct EpgIndex {
    by_channel: HashMap<String, Vec<Programme>>,
    /// Folded `<display-name>` → xmltv id, so a channel with no matching
    /// tvg-id (e.g. a Vavoo feed) can still resolve its guide by name.
    id_by_name: HashMap<String, String>,
}

impl EpgIndex {
    pub fn is_empty(&self) -> bool {
        self.by_channel.is_empty()
    }

    pub fn channel_count(&self) -> usize {
        self.by_channel.len()
    }

    pub fn contains(&self, xmltv_id: &str) -> bool {
        self.by_channel.contains_key(&xmltv_id.to_lowercase())
    }

    /// XMLTV id for a channel display name (folded via `channels::normalize`),
    /// if the guide lists a `<channel>` under that name. Only ids that carry
    /// programmes are returned — a name mapping to an empty schedule is useless.
    pub fn id_for_name(&self, display_name: &str) -> Option<&str> {
        let id = self
            .id_by_name
            .get(&super::channels::normalize(display_name))?;
        self.by_channel.contains_key(id).then_some(id.as_str())
    }

    /// Current + following programme on a channel. `next` is the first
    /// programme starting after `now` even when nothing airs right now
    /// (gap in the guide).
    pub fn now_next(
        &self,
        xmltv_id: &str,
        now: DateTime<Utc>,
    ) -> (Option<&Programme>, Option<&Programme>) {
        let Some(programmes) = self.by_channel.get(&xmltv_id.to_lowercase()) else {
            return (None, None);
        };
        // First programme starting strictly after `now`.
        let upcoming = programmes.partition_point(|p| p.start <= now);
        let current = upcoming
            .checked_sub(1)
            .map(|i| &programmes[i])
            .filter(|p| p.stop > now);
        (current, programmes.get(upcoming))
    }
}

/// Gunzip a fetched guide body to text, refusing more than `limit` bytes of
/// output (a gzip bomb would otherwise exhaust memory). Every member of a
/// multi-member file is read (concatenated `.gz` parts, `pigz` output).
pub fn decode_gzip(bytes: &[u8], limit: u64) -> std::io::Result<String> {
    let mut out = String::new();
    flate2::read::MultiGzDecoder::new(bytes)
        .take(limit.saturating_add(1))
        .read_to_string(&mut out)?;
    if u64::try_from(out.len()).unwrap_or(u64::MAX) > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "guide exceeds the decompressed size cap",
        ));
    }
    Ok(out)
}

/// Parse an XMLTV document, keeping programmes overlapping the
/// `[now - 36h, now + 48h]` window so a multi-day guide doesn't bloat
/// resident memory.
pub fn parse_xmltv(xml: &str, now: DateTime<Utc>) -> EpgIndex {
    let window_start = now - chrono::Duration::hours(36);
    let window_end = now + chrono::Duration::hours(48);

    // Not `trim_text(true)`: quick-xml splits element text at every entity
    // reference and trims each fragment, so "Tom &amp; Jerry" arrived as
    // "Tom" / GeneralRef / "Jerry". Fragments are joined and trimmed at End.
    let mut reader = Reader::from_str(xml);

    let mut index = EpgIndex::default();
    let mut buf = Vec::new();

    // State for the <programme> element being parsed.
    let mut current: Option<(String, Programme)> = None;
    // Which child element's text we're inside (title/desc/category/display-name).
    let mut field: Option<&'static str> = None;
    let mut text_acc = String::new();
    // Lowercased id of the <channel> element being parsed (for display-name).
    let mut current_channel_id: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                text_acc.clear();
                match e.name().as_ref() {
                    "channel" => {
                        current_channel_id = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == "id")
                            .map(|a| a.value.to_lowercase());
                    }
                    "display-name" if current_channel_id.is_some() => {
                        field = Some("display-name");
                    }
                    "programme" => current = programme_open(&e, window_start, window_end),
                    "title" if current.is_some() => field = Some("title"),
                    "desc" if current.is_some() => field = Some("desc"),
                    "category" if current.is_some() => field = Some("category"),
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if field.is_some() => text_acc.push_str(&t),
            Ok(Event::CData(c)) if field.is_some() => text_acc.push_str(&c),
            Ok(Event::GeneralRef(r)) if field.is_some() => {
                if let Some(ch) = resolve_general_ref(&r) {
                    text_acc.push(ch);
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                "programme" => {
                    if let Some((channel, prog)) = current.take()
                        && !prog.title.is_empty()
                    {
                        index.by_channel.entry(channel).or_default().push(prog);
                    }
                    field = None;
                }
                "channel" => current_channel_id = None,
                "title" | "desc" | "category" | "display-name" => {
                    if let Some(name) = field.take() {
                        apply_field(
                            &mut index,
                            name,
                            text_acc.trim(),
                            current_channel_id.as_ref(),
                            current.as_mut().map(|(_, p)| p),
                        );
                    }
                    text_acc.clear();
                }
                _ => {}
            },
            // EOF ends the parse; a mid-document error salvages what parsed
            // so far rather than dropping the whole guide.
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    for programmes in index.by_channel.values_mut() {
        programmes.sort_by_key(|p| p.start);
    }
    index
}

/// Store one finished text element: `<display-name>` folds name → id (first
/// wins), `<title>` keeps the last one, the first `<desc>` / `<category>` wins.
fn apply_field(
    index: &mut EpgIndex,
    field: &str,
    text: &str,
    channel_id: Option<&String>,
    prog: Option<&mut Programme>,
) {
    if text.is_empty() {
        return;
    }
    if field == "display-name" {
        if let Some(id) = channel_id {
            index
                .id_by_name
                .entry(super::channels::normalize(text))
                .or_insert_with(|| id.clone());
        }
        return;
    }
    let Some(prog) = prog else { return };
    match field {
        "title" => text.clone_into(&mut prog.title),
        "desc" if prog.description.is_none() => prog.description = Some(text.to_owned()),
        "category" if prog.category.is_none() => prog.category = Some(text.to_owned()),
        _ => {}
    }
}

/// An entity reference inside element text: numeric refs plus the five
/// predefined XML entities.
fn resolve_general_ref(r: &BytesRef<'_>) -> Option<char> {
    if let Ok(Some(ch)) = r.resolve_char_ref() {
        return Some(ch);
    }
    match &**r {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => None,
    }
}

/// Open a `<programme>` element into `(lowercase channel id, empty Programme)`
/// when its start/stop overlap the retained window, else `None`.
fn programme_open(
    e: &quick_xml::events::BytesStart,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
) -> Option<(String, Programme)> {
    let (mut channel, mut start, mut stop) = (None, None, None);
    for attr in e.attributes().flatten() {
        let value = &attr.value;
        match attr.key.as_ref() {
            "channel" => channel = Some(value.to_lowercase()),
            "start" => start = parse_xmltv_time(value),
            "stop" => stop = parse_xmltv_time(value),
            _ => {}
        }
    }
    let (channel, start, stop) = (channel?, start?, stop?);
    (stop > window_start && start < window_end).then(|| {
        (
            channel,
            Programme {
                start,
                stop,
                title: String::new(),
                category: None,
                description: None,
            },
        )
    })
}

/// XMLTV timestamps: `20260705203000 +0200`, the offset also met glued to
/// the time (`20260705203000+0200`), with a colon (`+02:00`) or absent, and
/// the seconds sometimes left out (`202607052030 +0200`). Naive times are
/// taken as UTC, which is what the format's spec implies for absent offsets
/// in practice.
fn parse_xmltv_time(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    let (stamp, offset) = s.split_at(digits);
    let format = match digits {
        14 => "%Y%m%d%H%M%S",
        12 => "%Y%m%d%H%M",
        _ => return None,
    };
    let naive = NaiveDateTime::parse_from_str(stamp, format).ok()?;
    let offset = match offset.trim() {
        "" | "Z" | "UTC" | "GMT" => return Some(naive.and_utc()),
        offset => FixedOffset::east_opt(offset_seconds(offset)?)?,
    };
    naive
        .and_local_timezone(offset)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
}

/// `+0200` / `-05:30` → seconds east of UTC.
fn offset_seconds(offset: &str) -> Option<i32> {
    let (sign, rest) = match offset.as_bytes().first()? {
        b'+' => (1, &offset[1..]),
        b'-' => (-1, &offset[1..]),
        _ => return None,
    };
    let hhmm = rest.replacen(':', "", 1);
    if hhmm.len() != 4 || !hhmm.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = hhmm[..2].parse().ok()?;
    let minutes: i32 = hhmm[2..].parse().ok()?;
    Some(sign * (hours * 3600 + minutes * 60))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    const GUIDE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tv generator-info-name="XML TV Fr">
  <channel id="TF1.fr"><display-name>TF1</display-name></channel>
  <channel id="Empty.fr"><display-name>Empty Chan</display-name></channel>
  <programme start="20260705200000 +0200" stop="20260705213000 +0200" channel="TF1.fr">
    <title lang="fr">Journal de 20h</title>
    <desc lang="fr">L'actualité du jour.</desc>
    <category lang="fr">News</category>
    <category lang="fr">Magazine</category>
  </programme>
  <programme start="20260705213000 +0200" stop="20260705231500 +0200" channel="TF1.fr">
    <title lang="fr">Film du dimanche</title>
  </programme>
  <programme start="20260706000000 +0200" stop="20260706010000 +0200" channel="France2.fr">
    <title>Untitled slot</title>
  </programme>
</tv>"#;

    #[test]
    fn parses_programmes_with_timezone() {
        // 2026-07-05 20:00 +0200 == 18:00 UTC
        let now = ts("2026-07-05T19:00:00Z");
        let index = parse_xmltv(GUIDE, now);
        assert_eq!(index.channel_count(), 2);
        assert!(index.contains("TF1.fr"));
        assert!(index.contains("tf1.FR"));

        let (current, next) = index.now_next("tf1.fr", now);
        let current = current.unwrap();
        assert_eq!(current.title, "Journal de 20h");
        assert_eq!(current.start, ts("2026-07-05T18:00:00Z"));
        assert_eq!(current.stop, ts("2026-07-05T19:30:00Z"));
        assert_eq!(current.category.as_deref(), Some("News"));
        assert_eq!(current.description.as_deref(), Some("L'actualité du jour."));
        assert_eq!(next.unwrap().title, "Film du dimanche");
    }

    #[test]
    fn now_next_boundaries() {
        let index = parse_xmltv(GUIDE, ts("2026-07-05T19:00:00Z"));
        // exactly at start → programme is current
        let (cur, _) = index.now_next("tf1.fr", ts("2026-07-05T18:00:00Z"));
        assert_eq!(cur.unwrap().title, "Journal de 20h");
        // exactly at stop → next programme is current (start == stop boundary)
        let (cur, _) = index.now_next("tf1.fr", ts("2026-07-05T19:30:00Z"));
        assert_eq!(cur.unwrap().title, "Film du dimanche");
        // after the last programme → nothing
        let (cur, next) = index.now_next("tf1.fr", ts("2026-07-06T00:00:00Z"));
        assert!(cur.is_none());
        assert!(next.is_none());
        // gap: before first programme → no current, but a next
        let (cur, next) = index.now_next("tf1.fr", ts("2026-07-05T12:00:00Z"));
        assert!(cur.is_none());
        assert_eq!(next.unwrap().title, "Journal de 20h");
        // unknown channel
        let (cur, next) = index.now_next("nope.fr", ts("2026-07-05T19:00:00Z"));
        assert!(cur.is_none() && next.is_none());
    }

    #[test]
    fn id_for_name_resolves_display_names_with_schedule() {
        let index = parse_xmltv(GUIDE, ts("2026-07-05T19:00:00Z"));
        // display-name folds (case-insensitive) → xmltv id
        assert_eq!(index.id_for_name("TF1"), Some("tf1.fr"));
        assert_eq!(index.id_for_name("tf1"), Some("tf1.fr"));
        // a <channel> with no in-window programmes gives no useful guide
        assert_eq!(index.id_for_name("Empty Chan"), None);
        // unknown name
        assert_eq!(index.id_for_name("Nope"), None);
    }

    #[test]
    fn entities_do_not_truncate_text() {
        let xml = r#"<tv>
  <channel id="C.fr"><display-name>France &amp; Co</display-name></channel>
  <programme start="20260705200000 +0000" stop="20260705210000 +0000" channel="C.fr">
    <title>Tom &amp; Jerry</title>
    <desc>L&apos;amour est dans le pr&#233;</desc>
    <category>Jeunesse &amp; dessins</category>
  </programme>
</tv>"#;
        let now = ts("2026-07-05T20:30:00Z");
        let index = parse_xmltv(xml, now);
        let (cur, _) = index.now_next("c.fr", now);
        let cur = cur.unwrap();
        assert_eq!(cur.title, "Tom & Jerry");
        assert_eq!(cur.description.as_deref(), Some("L'amour est dans le pré"));
        assert_eq!(cur.category.as_deref(), Some("Jeunesse & dessins"));
        assert_eq!(index.id_for_name("France & Co"), Some("c.fr"));
    }

    #[test]
    fn window_drops_far_past_and_future() {
        // "now" three days after the guide's content → everything expired
        let index = parse_xmltv(GUIDE, ts("2026-07-09T00:00:00Z"));
        assert!(index.is_empty());
    }

    #[test]
    fn xmltv_times_in_every_shape_met() {
        let at = |s: &str| parse_xmltv_time(s);
        let expected = Some(ts("2026-07-05T18:30:00Z"));
        assert_eq!(at("20260705203000 +0200"), expected);
        assert_eq!(at("20260705203000+0200"), expected, "glued offset");
        assert_eq!(at("20260705203000 +02:00"), expected, "colon offset");
        assert_eq!(at("202607052030 +0200"), expected, "no seconds");
        assert_eq!(at("202607052030+0200"), expected);
        assert_eq!(at("20260705183000"), expected, "naive is UTC");
        assert_eq!(at("20260705183000 Z"), expected);
        assert_eq!(at("20260705130000 -0530"), expected);
        for bad in [
            "",
            "2026070520",
            "20260705203000 +2",
            "20260705203000 0200",
            "2026-07-05",
        ] {
            assert_eq!(at(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_multi_member_gzip_reads_whole() {
        let (head, tail) = GUIDE.split_at(GUIDE.len() / 2);
        let mut gz = Vec::new();
        for part in [head, tail] {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(part.as_bytes()).unwrap();
            gz.extend(enc.finish().unwrap());
        }
        assert_eq!(decode_gzip(&gz, 1 << 20).unwrap(), GUIDE);
    }

    #[test]
    fn gzip_roundtrip() {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(GUIDE.as_bytes()).unwrap();
        let gz = enc.finish().unwrap();
        assert_eq!(decode_gzip(&gz, 1 << 20).unwrap(), GUIDE);
        assert!(decode_gzip(b"not gzip", 1 << 20).is_err());
        let exact = u64::try_from(GUIDE.len()).unwrap();
        assert_eq!(decode_gzip(&gz, exact).unwrap(), GUIDE);
        assert!(decode_gzip(&gz, exact - 1).is_err());
    }
}
