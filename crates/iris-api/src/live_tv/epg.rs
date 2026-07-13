//! XMLTV programme guide for Live TV's now/next overlay.
//!
//! The source (e.g. xmltvfr.fr) publishes a gzipped XMLTV document refreshed
//! daily. We gunzip explicitly (it is a gzipped *body* served as
//! `application/x-gzip`, not transport encoding — reqwest's gzip feature
//! never sees it), stream-parse with quick-xml, and keep a bounded window of
//! programmes per channel in memory.

use std::collections::HashMap;
use std::io::Read;

use chrono::{DateTime, NaiveDateTime, Utc};
use quick_xml::Reader;
use quick_xml::events::Event;

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

/// Gunzip a fetched guide body to text.
pub fn decode_gzip(bytes: &[u8]) -> std::io::Result<String> {
    let mut out = String::new();
    flate2::read::GzDecoder::new(bytes).read_to_string(&mut out)?;
    Ok(out)
}

/// Parse an XMLTV document, keeping programmes overlapping the
/// `[now - 36h, now + 48h]` window so a multi-day guide doesn't bloat
/// resident memory.
pub fn parse_xmltv(xml: &str, now: DateTime<Utc>) -> EpgIndex {
    let window_start = now - chrono::Duration::hours(36);
    let window_end = now + chrono::Duration::hours(48);

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut index = EpgIndex::default();
    let mut buf = Vec::new();

    // State for the <programme> element being parsed.
    let mut current: Option<(String, Programme)> = None;
    // Which child element's text we're inside (title/desc/category/display-name).
    let mut field: Option<&'static str> = None;
    // Lowercased id of the <channel> element being parsed (for display-name).
    let mut current_channel_id: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"channel" => {
                    current_channel_id = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"id")
                        .map(|a| String::from_utf8_lossy(&a.value).to_lowercase());
                }
                b"display-name" if current_channel_id.is_some() => field = Some("display-name"),
                b"programme" => current = programme_open(&e, window_start, window_end),
                b"title" if current.is_some() => field = Some("title"),
                b"desc" if current.is_some() => field = Some("desc"),
                b"category" if current.is_some() => field = Some("category"),
                _ => {}
            },
            Ok(Event::Text(t)) => {
                let text = || {
                    t.decode()
                        .map(std::borrow::Cow::into_owned)
                        .unwrap_or_default()
                };
                // <display-name> inside a <channel>: fold name → id (first wins).
                if field == Some("display-name")
                    && let Some(id) = &current_channel_id
                {
                    let name = text();
                    if !name.is_empty() {
                        index
                            .id_by_name
                            .entry(super::channels::normalize(&name))
                            .or_insert_with(|| id.clone());
                    }
                } else if let (Some(field), Some((_, prog))) = (field, current.as_mut()) {
                    let text = text();
                    if !text.is_empty() {
                        match field {
                            "title" => prog.title = text,
                            // first <desc> / <category> wins
                            "desc" if prog.description.is_none() => prog.description = Some(text),
                            "category" if prog.category.is_none() => prog.category = Some(text),
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"programme" => {
                    if let Some((channel, prog)) = current.take()
                        && !prog.title.is_empty()
                    {
                        index.by_channel.entry(channel).or_default().push(prog);
                    }
                    field = None;
                }
                b"channel" => current_channel_id = None,
                b"title" | b"desc" | b"category" | b"display-name" => field = None,
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

/// Open a `<programme>` element into `(lowercase channel id, empty Programme)`
/// when its start/stop overlap the retained window, else `None`.
fn programme_open(
    e: &quick_xml::events::BytesStart,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
) -> Option<(String, Programme)> {
    let (mut channel, mut start, mut stop) = (None, None, None);
    for attr in e.attributes().flatten() {
        let value = String::from_utf8_lossy(&attr.value);
        match attr.key.as_ref() {
            b"channel" => channel = Some(value.to_lowercase()),
            b"start" => start = parse_xmltv_time(&value),
            b"stop" => stop = parse_xmltv_time(&value),
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

/// XMLTV timestamps: `20260705203000 +0200` (offset optional; naive times
/// are taken as UTC, which is what the format's spec implies for absent
/// offsets in practice).
fn parse_xmltv_time(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y%m%d%H%M%S %z") {
        return Some(dt.with_timezone(&Utc));
    }
    NaiveDateTime::parse_from_str(s, "%Y%m%d%H%M%S")
        .ok()
        .map(|naive| naive.and_utc())
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
    fn window_drops_far_past_and_future() {
        // "now" three days after the guide's content → everything expired
        let index = parse_xmltv(GUIDE, ts("2026-07-09T00:00:00Z"));
        assert!(index.is_empty());
    }

    #[test]
    fn gzip_roundtrip() {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(GUIDE.as_bytes()).unwrap();
        let gz = enc.finish().unwrap();
        assert_eq!(decode_gzip(&gz).unwrap(), GUIDE);
        assert!(decode_gzip(b"not gzip").is_err());
    }
}
