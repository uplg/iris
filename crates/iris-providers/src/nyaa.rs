//! nyaa.si — public anime tracker.
//!
//! Nyaa has no Torznab endpoint, but it does publish every search as RSS
//! 2.0 with a `nyaa:` namespace carrying the fields Torznab would put in
//! `<torznab:attr>` (seeders, leechers, infohash, category, size). So the
//! wire format is close enough to reuse the same event-reader shape as
//! [`crate::torznab`], but different enough that composing `TorznabProvider`
//! would mean lying about the attribute syntax.
//!
//! Two properties make this the simplest provider in the tree:
//!
//! * **No auth.** Nyaa is public — no API key, no cookie session.
//! * **No link cache.** The download URL is derivable from the id
//!   (`/download/<id>.torrent`), so [`resolve`](NyaaProvider::resolve)
//!   reconstructs it instead of remembering what a previous `search()` saw.
//!   That makes grabs restart-safe for free, where the Torznab providers
//!   need a FIFO cache plus a persisted `download_url` to survive a deploy.
//!
//! Two config knobs exist for this provider specifically, both documented on
//! [`crate::registry::ProviderPolicy`]: `catalog = false` keeps its firehose
//! out of the discovery catalogue, and `seed = false` drops its grabs out of
//! the swarm on completion.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use iris_config::ProviderEntry;
use iris_core::search::{
    MediaKind, ProviderCapabilities, ProviderPage, SearchQuery, SearchResult, SortField, SortOrder,
    TorrentSource,
};
use iris_core::{Error, Result};
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::SearchProvider;
use crate::util::parse_size;

/// Items nyaa puts in one RSS page. Not configurable upstream.
const PAGE_SIZE: u32 = 75;

/// Default category filter: `1_0` is "Anime — all subcategories"
/// (english-translated, non-english-translated, raw, AMV). Narrowing it
/// (e.g. `1_2` for english-translated only) is a per-deployment taste
/// call, hence the config knob.
const DEFAULT_CATEGORY: &str = "1_0";

pub struct NyaaProvider {
    id: String,
    base_url: String,
    category: String,
    /// Nyaa's `f=` filter: 0 = no filter, 1 = no remakes, 2 = trusted only.
    filter: u8,
    http: reqwest::Client,
}

impl NyaaProvider {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<dyn SearchProvider>> {
        let base_url = entry
            .fields
            .get("base_url")
            .and_then(|v| v.as_str())
            .unwrap_or("https://nyaa.si")
            .trim_end_matches('/')
            .to_string();
        let category = entry
            .fields
            .get("categories")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_CATEGORY)
            .to_string();
        let filter = entry
            .fields
            .get("filter")
            .and_then(toml::Value::as_integer)
            .and_then(|n| u8::try_from(n).ok())
            .filter(|n| *n <= 2)
            .unwrap_or(0);

        let http = crate::tls::client_builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| Error::Provider(format!("nyaa http client: {e}")))?;

        Ok(Arc::new(Self {
            id: entry.id.clone(),
            base_url,
            category,
            filter,
            http,
        }))
    }

    fn rss_url(
        &self,
        q: &str,
        page: u32,
        sort: Option<SortField>,
        order: Option<SortOrder>,
    ) -> String {
        let mut url = format!(
            "{}/?page=rss&c={}&f={}&q={}",
            self.base_url,
            urlencode(&self.category),
            self.filter,
            urlencode(q),
        );
        if page > 1 {
            use std::fmt::Write as _;
            let _ = write!(url, "&p={page}");
        }
        // Nyaa's own sort keys. `Title` has no equivalent (the site sorts by
        // name only in the HTML view), so it falls through to nyaa's default
        // (`id desc` = newest first), which is also what `latest()` wants.
        if let Some(key) = sort.and_then(|s| match s {
            SortField::Size => Some("size"),
            SortField::Seeders => Some("seeders"),
            SortField::Leechers => Some("leechers"),
            SortField::Uploaded => Some("id"),
            SortField::Title => None,
        }) {
            use std::fmt::Write as _;
            let _ = write!(url, "&s={key}");
            url.push_str(match order {
                Some(SortOrder::Asc) => "&o=asc",
                _ => "&o=desc",
            });
        }
        url
    }

    async fn fetch(&self, url: &str) -> Result<String> {
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("nyaa get: {e}")))?
            .error_for_status()
            .map_err(|e| Error::Provider(format!("nyaa status: {e}")))?;
        resp.text()
            .await
            .map_err(|e| Error::Provider(format!("nyaa body: {e}")))
    }

    /// Build the query string nyaa's search engine sees. Nyaa tokenises on
    /// whitespace and ANDs the tokens, so a SCENE-style `S04E11` marker is a
    /// filter that no anime release name carries (they use absolute
    /// numbering: `One Piece - 1174`). Feeding it through would return zero
    /// results for every episode query, so we send the parsed TITLE when the
    /// caller gives us one and let the ranking layer sort out the episode.
    fn query_text(q: &SearchQuery) -> &str {
        match q.parsed_title.as_deref() {
            Some(t) if !t.trim().is_empty() => t,
            _ => q.q.as_str(),
        }
    }
}

#[async_trait]
impl SearchProvider for NyaaProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            returns_magnet: false,
            returns_torrent_file: true,
            returns_infohash: true,
        }
    }

    async fn search(&self, q: &SearchQuery) -> Result<ProviderPage> {
        let page = q.page.unwrap_or(1).max(1);
        let url = self.rss_url(Self::query_text(q), page, q.sort_by, q.order);
        let body = self.fetch(&url).await?;
        let items = parse_nyaa_rss(&body)?;
        let results: Vec<SearchResult> = items
            .into_iter()
            .filter_map(|item| item.into_result(&self.id, &self.base_url))
            .collect();
        Ok(ProviderPage {
            // A short page is the last page; a full one means "there may be
            // more". Nyaa's RSS carries no total, so that's all we can say.
            total_pages: (results.len() < PAGE_SIZE as usize).then_some(page),
            total_count: None,
            results,
            current_page: page,
            limit: PAGE_SIZE,
        })
    }

    async fn latest(&self, _kind: Option<MediaKind>, page: u32) -> Result<ProviderPage> {
        // Query-less RSS is nyaa's newest-first firehose — exactly the
        // rolling window `latest()` is defined as. `kind` is meaningless
        // here: every category nyaa indexes is anime.
        let page = page.max(1);
        let url = self.rss_url("", page, Some(SortField::Uploaded), Some(SortOrder::Desc));
        let body = self.fetch(&url).await?;
        let results: Vec<SearchResult> = parse_nyaa_rss(&body)?
            .into_iter()
            .filter_map(|item| item.into_result(&self.id, &self.base_url))
            .collect();
        Ok(ProviderPage {
            total_pages: (results.len() < PAGE_SIZE as usize).then_some(page),
            total_count: None,
            results,
            current_page: page,
            limit: PAGE_SIZE,
        })
    }

    async fn resolve(&self, external_id: &str) -> Result<TorrentSource> {
        if !external_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::Provider(format!(
                "nyaa external_id is not a torrent id: {external_id}"
            )));
        }
        let url = download_url(&self.base_url, external_id);
        let bytes = self.fetch_bytes(&url).await?;
        if bytes.first() != Some(&b'd') {
            return Err(Error::Provider(
                "nyaa download did not return a bencoded .torrent".into(),
            ));
        }
        Ok(TorrentSource::TorrentFile(bytes.to_vec()))
    }
}

fn download_url(base_url: &str, id: &str) -> String {
    format!("{base_url}/download/{id}.torrent")
}

/// Percent-encode a query-string value. Only the characters that would
/// break the URL — nyaa's search is happy with everything else verbatim.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => {
                use std::fmt::Write as _;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// One `<item>` of nyaa's RSS, before it becomes a [`SearchResult`].
#[derive(Default, Debug)]
struct RawItem {
    title: Option<String>,
    link: Option<String>,
    guid: Option<String>,
    pub_date: Option<String>,
    seeders: Option<u32>,
    leechers: Option<u32>,
    infohash: Option<String>,
    category: Option<String>,
    category_id: Option<String>,
    size: Option<String>,
}

impl RawItem {
    fn into_result(self, provider_id: &str, base_url: &str) -> Option<SearchResult> {
        let title = self.title?;
        // The id is what both URLs are built from: `<guid>` is
        // `https://nyaa.si/view/<id>`, `<link>` is
        // `https://nyaa.si/download/<id>.torrent`. Prefer the guid — it's the
        // canonical permalink — and fall back to the link so a feed that
        // omits one still yields a grabbable result.
        let id = self
            .guid
            .as_deref()
            .and_then(torrent_id)
            .or_else(|| self.link.as_deref().and_then(torrent_id))?;
        Some(SearchResult {
            provider_id: provider_id.to_string(),
            title: title.clone(),
            year: crate::util::extract_year(&title),
            size_bytes: self.size.as_deref().and_then(parse_size),
            seeders: self.seeders,
            leechers: self.leechers,
            infohash: self.infohash.map(|h| h.to_ascii_lowercase()),
            magnet: None,
            category: self.category,
            tags: Vec::new(),
            freeleech: false,
            uploader: None,
            uploaded_at: self.pub_date.as_deref().and_then(parse_rfc2822),
            tmdb_id: None,
            // Nyaa's taxonomy is "anime, by translation status" — it says
            // nothing about series vs film, so claiming a kind here would be
            // a guess. The SCENE parser downstream reads the release name,
            // which actually knows.
            kind: None,
            poster_url: None,
            already_in_library: false,
            library_infohash: None,
            library_file_idx: None,
            language: category_language(self.category_id.as_deref()).map(str::to_string),
            codec: None,
            download_url: Some(download_url(base_url, &id)),
            parsed_season: None,
            parsed_episode: None,
            external_id: id,
        })
    }
}

/// Language hint from nyaa's category, for releases whose TITLE carries no
/// tag. Nyaa splits its anime tree by translation status, which is exactly
/// the question the badge asks — but only `1_2` answers it unambiguously:
///
/// * `1_2` English-translated → english.
/// * `1_3` Non-English-translated → SOME language, and nyaa won't say which.
///   The French scene lives here (`… - VF VOSTFR - x264 AC3`), so the title's
///   own tag is the only trustworthy signal; claiming anything would badge
///   French rips as English.
/// * `1_4` Raw → untranslated Japanese, no badge applies.
/// * `1_1` AMV, `1_0` and the non-anime trees → nothing to say.
///
/// The API layer only consults this when the title parsed to `Unknown`, so an
/// explicit `VOSTFR` / `MULTi` in the name always wins over the category.
fn category_language(category_id: Option<&str>) -> Option<&'static str> {
    match category_id {
        Some("1_2") => Some("english"),
        _ => None,
    }
}

/// `https://nyaa.si/view/2148226` / `https://nyaa.si/download/2148226.torrent`
/// → `Some("2148226")`.
fn torrent_id(url: &str) -> Option<String> {
    let tail = url.rsplit('/').next()?;
    let digits: String = tail
        .trim_end_matches(".torrent")
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then_some(digits)
}

fn parse_rfc2822(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc2822(s.trim())
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// Elements we care about inside an `<item>`. Everything else is skipped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Title,
    Link,
    Guid,
    PubDate,
    Seeders,
    Leechers,
    InfoHash,
    Category,
    CategoryId,
    Size,
}

fn tag_of(name: &[u8]) -> Option<Tag> {
    match name {
        b"title" => Some(Tag::Title),
        b"link" => Some(Tag::Link),
        b"guid" => Some(Tag::Guid),
        b"pubDate" => Some(Tag::PubDate),
        b"nyaa:seeders" => Some(Tag::Seeders),
        b"nyaa:leechers" => Some(Tag::Leechers),
        b"nyaa:infoHash" => Some(Tag::InfoHash),
        b"nyaa:category" => Some(Tag::Category),
        b"nyaa:categoryId" => Some(Tag::CategoryId),
        b"nyaa:size" => Some(Tag::Size),
        _ => None,
    }
}

fn apply(item: &mut RawItem, tag: Tag, text: String) {
    match tag {
        Tag::Title => item.title = Some(text),
        Tag::Link => item.link = Some(text),
        Tag::Guid => item.guid = Some(text),
        Tag::PubDate => item.pub_date = Some(text),
        Tag::Seeders => item.seeders = text.parse().ok(),
        Tag::Leechers => item.leechers = text.parse().ok(),
        Tag::InfoHash => item.infohash = Some(text),
        Tag::Category => item.category = Some(text),
        Tag::CategoryId => item.category_id = Some(text),
        Tag::Size => item.size = Some(text),
    }
}

/// Parse nyaa's RSS into raw items.
///
/// Character data is accumulated and flushed at the element's `End` rather
/// than applied per event: quick-xml splits element content at every entity
/// reference, so a title containing `&` (`[Group] Fate&#47;Zero`) arrives as
/// several Text/GeneralRef events and applying each would keep only the last
/// fragment. Same failure mode `torznab.rs` documents.
fn parse_nyaa_rss(body: &str) -> Result<Vec<RawItem>> {
    let mut reader = Reader::from_str(body);
    // Deliberately NOT `trim_text(true)`: quick-xml splits element content at
    // every entity reference, and trimming each fragment eats the spaces
    // AROUND the entity — `Fate &amp; Zero` came back as `Fate&Zero`, which
    // then fails to match anything downstream. Accumulate raw and trim once,
    // at the element's End.

    let mut items = Vec::new();
    let mut current: Option<RawItem> = None;
    let mut tag: Option<Tag> = None;
    let mut text = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                text.clear();
                if e.name().as_ref() == b"item" {
                    current = Some(RawItem::default());
                    tag = None;
                } else if current.is_some() {
                    tag = tag_of(e.name().as_ref());
                }
            }
            Ok(Event::Text(t)) => {
                if current.is_some()
                    && tag.is_some()
                    && let Ok(s) = t.decode()
                {
                    text.push_str(&s);
                }
            }
            Ok(Event::CData(c)) => {
                // CDATA is literal — no entity unescaping.
                if current.is_some()
                    && tag.is_some()
                    && let Ok(s) = std::str::from_utf8(c.as_ref())
                {
                    text.push_str(s);
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if current.is_some()
                    && tag.is_some()
                    && let Some(ch) = resolve_ref(r.as_ref())
                {
                    text.push(ch);
                }
            }
            Ok(Event::End(e)) => {
                if let (Some(item), Some(t)) = (current.as_mut(), tag) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        apply(item, t, trimmed.to_string());
                    }
                }
                text.clear();
                tag = None;
                if e.name().as_ref() == b"item"
                    && let Some(item) = current.take()
                {
                    items.push(item);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Provider(format!("nyaa rss: {e}"))),
            _ => {}
        }
        buf.clear();
    }
    Ok(items)
}

/// Resolve an XML entity reference body (the bytes between `&` and `;`).
fn resolve_ref(raw: &[u8]) -> Option<char> {
    let s = std::str::from_utf8(raw).ok()?;
    match s {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let hex = s.strip_prefix("#x").or_else(|| s.strip_prefix("#X"));
            let code = match hex {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => s.strip_prefix('#')?.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed capture of a real `https://nyaa.si/?page=rss&q=one+piece`
    /// response (2026-08).
    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<rss xmlns:atom="http://www.w3.org/2005/Atom" xmlns:nyaa="https://nyaa.si/xmlns/nyaa" version="2.0">
	<channel>
		<title>Nyaa - &#34;one piece&#34; - Torrent File RSS</title>
		<link>https://nyaa.si/</link>
		<item>
			<title>[Judas] One Piece - 1174 [1080p][HEVC x265 10bit][Multi-Subs] (Weekly)</title>
			<link>https://nyaa.si/download/2148226.torrent</link>
			<guid isPermaLink="true">https://nyaa.si/view/2148226</guid>
			<pubDate>Tue, 18 Aug 2026 22:11:01 -0000</pubDate>
			<nyaa:seeders>208</nyaa:seeders>
			<nyaa:leechers>1</nyaa:leechers>
			<nyaa:downloads>1464</nyaa:downloads>
			<nyaa:infoHash>EF008EB3770377839FB32B48D5315859B07194EA</nyaa:infoHash>
			<nyaa:categoryId>1_2</nyaa:categoryId>
			<nyaa:category>Anime - English-translated</nyaa:category>
			<nyaa:size>406.3 MiB</nyaa:size>
			<nyaa:trusted>No</nyaa:trusted>
			<nyaa:remake>Yes</nyaa:remake>
			<description><![CDATA[<a href="https://nyaa.si/view/2148226">#2148226</a>]]></description>
		</item>
		<item>
			<title>Pandora Hearts - CUSTOM DVDRIP - VF VOSTFR - x264 AC3</title>
			<link>https://nyaa.si/download/2148300.torrent</link>
			<guid isPermaLink="true">https://nyaa.si/view/2148300</guid>
			<nyaa:seeders>3</nyaa:seeders>
			<nyaa:categoryId>1_3</nyaa:categoryId>
			<nyaa:category>Anime - Non-English-translated</nyaa:category>
			<nyaa:size>7.2 GiB</nyaa:size>
		</item>
		<item>
			<title>[Erai-raws] Fate &amp; Zero - 01 [1080p]</title>
			<link>https://nyaa.si/download/2148201.torrent</link>
			<guid isPermaLink="true">https://nyaa.si/view/2148201</guid>
			<pubDate>Tue, 18 Aug 2026 20:06:09 -0000</pubDate>
			<nyaa:seeders>194</nyaa:seeders>
			<nyaa:leechers>2</nyaa:leechers>
			<nyaa:infoHash>fa267a3b333394b7269aab10c82f7e25b5d0a925</nyaa:infoHash>
			<nyaa:category>Anime - English-translated</nyaa:category>
			<nyaa:size>658.9 MiB</nyaa:size>
		</item>
	</channel>
</rss>"#;

    fn results() -> Vec<SearchResult> {
        parse_nyaa_rss(SAMPLE)
            .expect("sample parses")
            .into_iter()
            .filter_map(|i| i.into_result("nyaa", "https://nyaa.si"))
            .collect()
    }

    #[test]
    fn parses_items_with_nyaa_namespace() {
        let r = results();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].external_id, "2148226");
        assert_eq!(
            r[0].title,
            "[Judas] One Piece - 1174 [1080p][HEVC x265 10bit][Multi-Subs] (Weekly)"
        );
        assert_eq!(r[0].seeders, Some(208));
        assert_eq!(r[0].leechers, Some(1));
        assert_eq!(
            r[0].size_bytes,
            Some(426_036_429),
            "406.3 MiB, binary multiplier"
        );
        assert_eq!(r[0].category.as_deref(), Some("Anime - English-translated"));
        assert_eq!(
            r[0].infohash.as_deref(),
            Some("ef008eb3770377839fb32b48d5315859b07194ea"),
            "infohashes are normalised to lowercase for librqbit"
        );
        assert_eq!(
            r[0].download_url.as_deref(),
            Some("https://nyaa.si/download/2148226.torrent"),
            "download URL is derived from the id, not remembered from the feed"
        );
        assert!(r[0].uploaded_at.is_some());
    }

    /// The failure mode `torznab.rs` hit: quick-xml splits element text at
    /// every entity reference, so applying fragments as they land keeps only
    /// the last one and truncates the title.
    #[test]
    fn entity_references_in_titles_survive() {
        assert_eq!(results()[2].title, "[Erai-raws] Fate & Zero - 01 [1080p]");
    }

    /// Nyaa is a MIXED-language tracker: `1_2` is english-translated, `1_3`
    /// is "translated into something, we won't say what" — which is where the
    /// French scene lives. A blanket provider default would badge those rips
    /// English, so the hint is per-category and only ever emitted for `1_2`.
    #[test]
    fn language_hint_only_for_english_translated_category() {
        let r = results();
        assert_eq!(r[0].language.as_deref(), Some("english"), "1_2");
        assert_eq!(
            r[1].language, None,
            "1_3 could be French, Spanish, … — only the title's own tag can say"
        );
        assert_eq!(category_language(Some("1_4")), None, "raw = untranslated");
        assert_eq!(category_language(None), None);
    }

    #[test]
    fn torrent_id_reads_both_url_shapes() {
        assert_eq!(
            torrent_id("https://nyaa.si/view/2148226").as_deref(),
            Some("2148226")
        );
        assert_eq!(
            torrent_id("https://nyaa.si/download/2148226.torrent").as_deref(),
            Some("2148226")
        );
        assert_eq!(torrent_id("https://nyaa.si/"), None);
    }

    /// Anime releases use absolute numbering (`One Piece - 1174`), never
    /// `SxxExx` — sending the raw SCENE query would AND in a token no title
    /// carries and return nothing.
    #[test]
    fn query_uses_parsed_title_when_available() {
        let q = SearchQuery {
            q: "One Piece S01E1174".into(),
            parsed_title: Some("one piece".into()),
            season: Some(1),
            episode: Some(1174),
            page: None,
            limit: None,
            sort_by: None,
            order: None,
            kind: None,
            year: None,
        };
        assert_eq!(NyaaProvider::query_text(&q), "one piece");

        let raw = SearchQuery {
            parsed_title: None,
            ..q
        };
        assert_eq!(NyaaProvider::query_text(&raw), "One Piece S01E1174");
    }

    #[test]
    fn rss_url_encodes_query_and_pages() {
        let p = NyaaProvider {
            id: "nyaa".into(),
            base_url: "https://nyaa.si".into(),
            category: "1_0".into(),
            filter: 0,
            http: reqwest::Client::new(),
        };
        assert_eq!(
            p.rss_url("cowboy bebop", 1, None, None),
            "https://nyaa.si/?page=rss&c=1_0&f=0&q=cowboy+bebop"
        );
        assert_eq!(
            p.rss_url("k-on!", 3, Some(SortField::Seeders), Some(SortOrder::Desc)),
            "https://nyaa.si/?page=rss&c=1_0&f=0&q=k-on%21&p=3&s=seeders&o=desc"
        );
    }

    /// Live end-to-end check against nyaa (network) — run explicitly with
    /// `cargo test -p iris-providers nyaa_live -- --ignored`.
    #[tokio::test]
    #[ignore = "hits the live nyaa.si API"]
    async fn nyaa_live_search_and_resolve() {
        let entry = ProviderEntry {
            id: "nyaa".into(),
            kind: "nyaa".into(),
            enabled: true,
            fields: std::collections::HashMap::new(),
        };
        let p = NyaaProvider::from_config(&entry).expect("provider builds");
        let page = p
            .search(&SearchQuery {
                q: "one piece".into(),
                page: Some(1),
                limit: None,
                sort_by: Some(SortField::Seeders),
                order: Some(SortOrder::Desc),
                kind: None,
                parsed_title: None,
                season: None,
                episode: None,
                year: None,
            })
            .await
            .expect("search succeeds");
        assert!(!page.results.is_empty(), "one piece should have results");
        let first = &page.results[0];
        eprintln!(
            "nyaa: {} results, first = {:?} ({} seeders)",
            page.results.len(),
            first.title,
            first.seeders.unwrap_or(0),
        );
        let source = p.resolve(&first.external_id).await.expect("resolve");
        match source {
            TorrentSource::TorrentFile(bytes) => {
                assert_eq!(bytes.first(), Some(&b'd'), "bencoded .torrent expected");
            }
            TorrentSource::Magnet(m) => panic!("expected a .torrent file, got magnet {m}"),
        }
    }
}
