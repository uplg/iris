//! Pure parsers for dlive.sx pages and the third-party embeds behind its
//! players. No I/O: every function takes a body already fetched.

use std::collections::HashSet;

use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

/// Standard alphabet, padding optional — the embeds strip it.
const B64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);
const B64_URL: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedChannel {
    pub id: u32,
    pub name: String,
}

/// The cards of `24-7-channels.php`, in page order. The body is not valid
/// UTF-8 (truncated multi-byte `data-first` letters), so it is decoded
/// lossily; names are entity-decoded, adult entries dropped, and a duplicated
/// id keeps its first card.
pub fn channel_list(body: &[u8]) -> Vec<ListedChannel> {
    let html = String::from_utf8_lossy(body);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut rest: &str = &html;
    while let Some(start) = rest.find("<a class=\"card\"") {
        let card = &rest[start..];
        let end = card.find("</a>").map_or(card.len(), |e| e + 4);
        let block = &card[..end];
        rest = &card[end..];
        let Some(id) =
            between(block, "watch.php?id=", "\"").and_then(|v| v.trim().parse::<u32>().ok())
        else {
            continue;
        };
        let raw_name = between(block, "card__title\">", "<")
            .or_else(|| between(block, "data-title=\"", "\""))
            .unwrap_or("");
        let name = decode_entities(raw_name.trim());
        if name.is_empty() || is_adult(&name) || !seen.insert(id) {
            continue;
        }
        out.push(ListedChannel { id, name });
    }
    out
}

pub(super) fn is_adult(name: &str) -> bool {
    name.contains("18+")
}

/// Text between the first `open` and the next `close` after it.
fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = s.find(open)? + open.len();
    let len = s[start..].find(close)?;
    Some(&s[start..start + len])
}

/// The `src` of the page's player iframe: the first iframe that is not a
/// tracking pixel.
pub fn player_iframe(html: &str) -> Option<String> {
    let mut rest = html;
    while let Some(start) = rest.find("<iframe") {
        let tag_start = &rest[start..];
        let end = tag_start.find('>').unwrap_or(tag_start.len());
        let tag = &tag_start[..end];
        rest = &tag_start[end..];
        let Some(src) = attr(tag, "src") else {
            continue;
        };
        let src = decode_entities(src.trim());
        if src.is_empty() || src.starts_with("about:") || src.contains("histats") {
            continue;
        }
        return Some(src);
    }
    None
}

/// Value of a quoted attribute inside one tag (`name="…"` or `name='…'`),
/// only where `name` starts an attribute.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(pos) = tag[from..].find(name) {
        let at = from + pos;
        from = at + name.len();
        let before = tag[..at].chars().next_back();
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let after = tag[from..].trim_start();
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let value = &after[1..];
        return value.find(quote).map(|e| &value[..e]);
    }
    None
}

/// Player 1's playlist URL: `const SRC = "…";` in the embed's inline script.
pub fn const_src(html: &str) -> Option<String> {
    between(html, "const SRC = \"", "\"")
        .map(str::trim)
        .filter(|s| s.starts_with("http"))
        .map(str::to_string)
}

/// Player 1's playlist URL for every id, from one learned URL:
/// `…/premium950/index.m3u8` → `…/premium{id}/index.m3u8`.
pub fn edge_template(src: &str, probe_id: u32) -> Option<String> {
    let needle = format!("premium{probe_id}/");
    src.contains(&needle)
        .then(|| src.replacen(&needle, "premium{id}/", 1))
}

/// The two stream URLs of an `_econfig` embed (assetrage, lunchup…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Econfig {
    pub stream_url: String,
    pub stream_url_nop2p: Option<String>,
}

/// Whether the page carries an `_econfig` at all (a decode failure on such a
/// page means the format changed, not that the channel is missing).
pub fn has_econfig(html: &str) -> bool {
    html.contains("window._econfig='")
}

/// Decode `window._econfig`: base64, cut in four equal chunks, drop each
/// chunk's 4th character, base64-decode each, put them back in the order
/// `[2, 0, 3, 1]`, base64-decode the join, JSON-parse.
pub fn econfig(html: &str) -> Option<Econfig> {
    let raw = between(html, "window._econfig='", "'")?;
    let outer = B64.decode(raw.trim()).ok()?;
    let n = outer.len().div_ceil(4);
    let mut parts: [Vec<u8>; 4] = Default::default();
    for (i, pos) in [2usize, 0, 3, 1].into_iter().enumerate() {
        let chunk = outer.get(i * n..((i + 1) * n).min(outer.len()))?;
        if chunk.len() < 4 {
            return None;
        }
        let mut kept = chunk[..3].to_vec();
        kept.extend_from_slice(&chunk[4..]);
        parts[pos] = B64.decode(&kept).ok()?;
    }
    let joined: Vec<u8> = parts.concat();
    let json = B64.decode(&joined).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&json).ok()?;
    let stream_url = value.get("stream_url")?.as_str()?.to_string();
    if !stream_url.starts_with("http") {
        return None;
    }
    let stream_url_nop2p = value
        .get("stream_url_nop2p")
        .and_then(serde_json::Value::as_str)
        .filter(|s| s.starts_with("http") && *s != stream_url)
        .map(str::to_string);
    Some(Econfig {
        stream_url,
        stream_url_nop2p,
    })
}

/// The `const config = {…}` of a wideiptv player page (Player 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WideConfig {
    pub stream_url: String,
    pub slug: String,
    pub token: String,
    pub expires_in: u64,
    pub refresh_url: Option<String>,
}

pub fn wide_config(html: &str) -> Option<WideConfig> {
    let js_string = |key: &str| {
        let raw = between(html, &format!("{key}: \""), "\"")?;
        serde_json::from_str::<String>(&format!("\"{raw}\"")).ok()
    };
    let stream_url = js_string("streamUrl").filter(|s| s.starts_with("http"))?;
    let token = js_string("currentToken")
        .or_else(|| query_param(&stream_url, "token").map(str::to_string))?;
    let slug = js_string("channelSlug").unwrap_or_default();
    let expires_in = between(html, "tokenExpiresIn: ", ",")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(300);
    let refresh_url = between(html, "fetch(`", "`")
        .filter(|u| u.contains("refresh") && u.starts_with("http"))
        .map(str::to_string);
    Some(WideConfig {
        stream_url,
        slug,
        token,
        expires_in,
        refresh_url,
    })
}

/// Unix expiry inside a wideiptv token:
/// `base64("France2|no_check_ip|1791306618").<hmac>`.
pub fn wide_token_expiry(token: &str) -> Option<i64> {
    let head = token.split('.').next()?;
    let bytes = B64.decode(head).or_else(|_| B64_URL.decode(head)).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    text.rsplit('|').next()?.trim().parse().ok()
}

#[derive(Debug, serde::Deserialize)]
pub struct RefreshReply {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

/// One raw query parameter's value (no percent-decoding — tokens and
/// signatures are compared and substituted as they were minted).
pub fn query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let query = url.split_once('?')?.1;
    let query = query.split('#').next().unwrap_or(query);
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
}

/// `url` with one raw query parameter's value replaced, every other byte
/// kept. `None` when the parameter is absent.
pub fn replace_query_param(url: &str, key: &str, value: &str) -> Option<String> {
    let (base, rest) = url.split_once('?')?;
    let (query, fragment) = rest
        .split_once('#')
        .map_or((rest, None), |(q, f)| (q, Some(f)));
    let mut found = false;
    let pairs: Vec<String> = query
        .split('&')
        .map(
            |kv| match kv.strip_prefix(key).and_then(|r| r.strip_prefix('=')) {
                Some(_) => {
                    found = true;
                    format!("{key}={value}")
                }
                None => kv.to_string(),
            },
        )
        .collect();
    if !found {
        return None;
    }
    let mut out = format!("{base}?{}", pairs.join("&"));
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    Some(out)
}

/// Decode the HTML entities dlive's names carry (`&#039;`, `&amp;`…).
/// Unknown entities stay as written.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let decoded = tail.find(';').filter(|&semi| semi <= 10).and_then(|semi| {
            let name = &tail[1..semi];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .map(|hex| u32::from_str_radix(hex, 16))
                    .or_else(|| name.strip_prefix('#').map(str::parse::<u32>))
                    .and_then(Result::ok)
                    .and_then(char::from_u32),
            }?;
            Some((c, semi + 1))
        });
        if let Some((c, used)) = decoded {
            out.push(c);
            rest = &tail[used..];
        } else {
            out.push('&');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Country words dlive appends to a channel name (lowercase), and the Iris
/// country code each one names. Read off the real list: the suffixes it
/// actually carries.
const COUNTRY_WORDS: &[(&str, &str)] = &[
    ("france", "fr"),
    ("fr", "fr"),
    ("uk", "gb"),
    ("england", "gb"),
    ("gb", "gb"),
    ("usa", "us"),
    ("us", "us"),
    ("de", "de"),
    ("germany", "de"),
    ("deutschland", "de"),
    ("italy", "it"),
    ("it", "it"),
    ("italia", "it"),
    ("spain", "es"),
    ("es", "es"),
    ("españa", "es"),
    ("espana", "es"),
    ("portugal", "pt"),
    ("pt", "pt"),
    ("nl", "nl"),
    ("netherlands", "nl"),
    ("netherland", "nl"),
    ("holland", "nl"),
    ("belgium", "be"),
    ("be", "be"),
    ("canada", "ca"),
    ("ca", "ca"),
    ("australia", "au"),
    ("au", "au"),
    ("ireland", "ie"),
    ("ie", "ie"),
    ("poland", "pl"),
    ("pl", "pl"),
    ("cz", "cz"),
    ("sk", "sk"),
    ("israel", "il"),
    ("bulgaria", "bg"),
    ("denmark", "dk"),
    ("serbia", "rs"),
    ("mexico", "mx"),
    ("mx", "mx"),
    ("greece", "gr"),
    ("croatia", "hr"),
    ("turkey", "tr"),
    ("tr", "tr"),
    ("brasil", "br"),
    ("brazil", "br"),
    ("br", "br"),
    ("nz", "nz"),
    ("argentina", "ar"),
    ("ar", "ar"),
    ("romania", "ro"),
    ("cyprus", "cy"),
    ("uae", "ae"),
    ("russia", "ru"),
    ("malaysia", "my"),
    ("sweden", "se"),
    ("norway", "no"),
    ("pk", "pk"),
    ("qatar", "qa"),
    ("bih", "ba"),
];

/// The Iris country a lowercase word names, `uk` read as `gb`.
fn word_country(word: &str) -> Option<&'static str> {
    COUNTRY_WORDS
        .iter()
        .find(|(w, _)| *w == word)
        .map(|(_, code)| *code)
}

fn same_country(a: &str, b: &str) -> bool {
    fn canon(c: &str) -> &str {
        if c == "uk" { "gb" } else { c }
    }
    canon(a) == canon(b)
}

/// The trailing country word of a dlive name, as `(its token index, the
/// country it names)`, past an `HD` marker. Not one after a preposition
/// (`"Sport en France"`), nor a name's only word (`"France"`).
fn trailing_country(tokens: &[&str]) -> Option<(usize, &'static str)> {
    let mut end = tokens.len();
    while end >= 2 && tokens[end - 1].eq_ignore_ascii_case("hd") {
        end -= 1;
    }
    if end < 2 {
        return None;
    }
    let code = word_country(&tokens[end - 1].to_lowercase())?;
    let before = tokens[end - 2].to_lowercase();
    if matches!(
        before.as_str(),
        "en" | "de" | "du" | "in" | "of" | "la" | "le"
    ) {
        return None;
    }
    Some((end - 1, code))
}

/// The country a dlive name restricts its channel to, when it ends with a
/// country word (`"TF1 France"` → `fr`, `"RTL DE"` → `de`).
pub fn name_country(raw: &str) -> Option<&'static str> {
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    trailing_country(&tokens).map(|(_, code)| code)
}

/// Whether a dlive channel named `raw` may join a channel list of `country`:
/// any, unless its trailing country word names another one.
pub fn fits_country(raw: &str, country: &str) -> bool {
    name_country(raw).is_none_or(|code| same_country(code, country))
}

/// Display name for a dlive channel in `country`: the trailing country word
/// (when it names `country`) and an `HD` marker go, so `"TF1 France"` folds
/// onto iptv-org's `TF1` and `"L'Equipe France"` onto the TNT 21 aliases. A
/// country word after a preposition stays (`"Sport en France"`).
pub fn clean_name(raw: &str, country: &str) -> String {
    let mut tokens: Vec<&str> = raw.split_whitespace().collect();
    if let Some((at, code)) = trailing_country(&tokens)
        && same_country(code, country)
    {
        tokens.truncate(at);
    }
    while tokens.len() >= 2 && tokens.last().is_some_and(|t| t.eq_ignore_ascii_case("hd")) {
        tokens.pop();
    }
    let name = tokens.join(" ");
    let key = super::super::channels::normalize(&name);
    NAME_ALIASES
        .iter()
        .find(|(from, _)| *from == key)
        .map_or(name, |(_, to)| (*to).to_string())
}

/// dlive names whose fold differs from the iptv-org identity they carry
/// (folded dlive name → a name folding onto that identity): dlive spells
/// `RTÉ One` as `RTE 1`.
const NAME_ALIASES: &[(&str, &str)] = &[("rte1", "RTÉ One")];

/// Category for a dlive-only channel (no iptv-org counterpart to inherit one
/// from): dlive mostly adds pay sports channels.
pub fn category_for(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    [
        "sport", "bein", "dazn", "foot", "motogp", "formula", "equipe",
    ]
    .iter()
    .any(|w| lower.contains(w))
    .then_some("Sports")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &[u8] = include_bytes!("fixtures/list.html");

    #[test]
    fn channel_list_decodes_lossily_drops_adult_and_duplicates() {
        assert!(
            String::from_utf8_lossy(LIST).contains('\u{FFFD}'),
            "the fixture keeps dlive's invalid UTF-8 byte"
        );
        let list = channel_list(LIST);
        let name = |id: u32| list.iter().find(|c| c.id == id).map(|c| c.name.as_str());
        assert_eq!(name(469), Some("TF1 France"));
        assert_eq!(name(950), Some("France 2"));
        assert_eq!(name(645), Some("L'Equipe France"), "&#039; decoded");
        assert!(list.iter().any(|c| c.name == "A&E USA"), "&amp; decoded");
        assert_eq!(name(395), Some("МАТЧ! БОЕЦ Russia"), "valid UTF-8 kept");
        assert!(list.iter().all(|c| !c.name.contains("18+")));
        assert_eq!(list.iter().filter(|c| c.id == 811).count(), 1);
        assert_eq!(list.len(), 18);
        assert_eq!(channel_list(b"<html>moved</html>"), Vec::new());
    }

    #[test]
    fn player_pages_yield_their_player_iframe() {
        assert_eq!(
            player_iframe(include_str!("fixtures/stream-950.html")).as_deref(),
            Some("https://dembed.top/premiumtv/daddy.php?id=950")
        );
        assert_eq!(
            player_iframe(include_str!("fixtures/cast-950.html")).as_deref(),
            Some("https://assetrage.net/e/b1k4trwm0x1t6")
        );
        assert_eq!(
            player_iframe(include_str!("fixtures/player-950.html")).as_deref(),
            Some("https://wideiptv.top/daddy.php?stream=France2")
        );
        assert_eq!(
            player_iframe(include_str!("fixtures/wideiptv-daddy.html")).as_deref(),
            Some("https://wideiptv.top/player/France2"),
            "src after other attributes"
        );
        assert_eq!(player_iframe("<iframe data-src=\"x\"></iframe>"), None);
        assert_eq!(
            player_iframe("<iframe src='/e/a&amp;b'>").as_deref(),
            Some("/e/a&b")
        );
    }

    #[test]
    fn const_src_learns_the_edge_template() {
        let src = const_src(include_str!("fixtures/dembed-950.html")).unwrap();
        assert_eq!(src, "https://edge.cowedd4855ws.sbs/premium950/index.m3u8");
        let tpl = edge_template(&src, 950).unwrap();
        assert_eq!(tpl, "https://edge.cowedd4855ws.sbs/premium{id}/index.m3u8");
        assert_eq!(edge_template(&src, 51), None);
        assert_eq!(const_src("const SRC = \"\";"), None);
    }

    #[test]
    fn econfig_decodes_the_real_sample() {
        let html = include_str!("fixtures/assetrage-950.html");
        assert!(has_econfig(html));
        let cfg = econfig(html).unwrap();
        assert_eq!(
            cfg.stream_url,
            "https://e8975o.7odxv0l067ka.net:8443/hls/pq1gxvz9ymj74.m3u8?s=VG6hZoeJ8bTx1UTawwLd1g&e=1791327573"
        );
        assert_eq!(cfg.stream_url_nop2p, None, "same URL twice is one URL");
        assert_eq!(query_param(&cfg.stream_url, "e"), Some("1791327573"));
        assert_eq!(econfig("window._econfig='bm90IGEgY29uZmln'"), None);
        assert!(!has_econfig("<h4>Not allowed</h4>"));
    }

    #[test]
    fn wideiptv_page_and_token() {
        let cfg = wide_config(include_str!("fixtures/wideiptv-player.html")).unwrap();
        assert_eq!(
            cfg.stream_url,
            "https://ds167.bluetier.top/France2/index.m3u8?token=RnJhbmNlMnxub19jaGVja19pcHwxNzkxMzA2NjE4.c5352444506b29226dd2ffb94bf77a81a894423e27550cb4cd0d04e3097bcd3b"
        );
        assert_eq!(cfg.slug, "France2");
        assert_eq!(cfg.expires_in, 300);
        assert_eq!(
            cfg.refresh_url.as_deref(),
            Some("https://wideiptv.top/api/refresh_token.php")
        );
        assert_eq!(wide_token_expiry(&cfg.token), Some(1_791_306_618));
        assert_eq!(wide_token_expiry("garbage"), None);

        let reply: RefreshReply =
            serde_json::from_str(include_str!("fixtures/refresh_token.json")).unwrap();
        assert!(reply.success);
        let token = reply.token.unwrap();
        assert_eq!(reply.expires_in, Some(300));
        assert_eq!(wide_token_expiry(&token), Some(1_791_307_008));
        let renewed = replace_query_param(&cfg.stream_url, "token", &token).unwrap();
        assert!(renewed.ends_with(&format!("index.m3u8?token={token}")));
    }

    #[test]
    fn query_params_are_matched_whole() {
        let tiktok = "https://p16.example/x.image?dr=1&refresh_token=de00&x-expires=2106662400";
        assert_eq!(query_param(tiktok, "token"), None);
        assert_eq!(replace_query_param(tiktok, "token", "new"), None);
        assert_eq!(
            replace_query_param("https://h/a.ts?a=1&token=old&b=2#f", "token", "new").as_deref(),
            Some("https://h/a.ts?a=1&token=new&b=2#f")
        );
    }

    #[test]
    fn entities() {
        assert_eq!(decode_entities("L&#039;Equipe"), "L'Equipe");
        assert_eq!(decode_entities("A&amp;E &#x41;&lt;"), "A&E A<");
        assert_eq!(decode_entities("R&B & more &bogus;"), "R&B & more &bogus;");
    }

    #[test]
    fn names_fold_onto_the_iptv_org_identities() {
        assert_eq!(clean_name("TF1 France", "fr"), "TF1");
        assert_eq!(clean_name("BFM TV France", "fr"), "BFM TV");
        assert_eq!(clean_name("L'Equipe France", "fr"), "L'Equipe");
        assert_eq!(clean_name("beIN SPORTS 1 France", "fr"), "beIN SPORTS 1");
        assert_eq!(clean_name("Canal+ Sport France", "fr"), "Canal+ Sport");
        assert_eq!(clean_name("France 2", "fr"), "France 2");
        assert_eq!(clean_name("Sport en France", "fr"), "Sport en France");
        assert_eq!(clean_name("France", "fr"), "France");
        assert_eq!(clean_name("Sky Cinema Hits UK", "gb"), "Sky Cinema Hits");
        assert_eq!(clean_name("TF1 France", "gb"), "TF1 France");
        assert_eq!(clean_name("TF1 France HD", "fr"), "TF1");
        assert_eq!(name_country("RTL DE"), Some("de"));
        assert_eq!(name_country("RTL7 Netherland"), Some("nl"));
        assert_eq!(name_country("Sport en France"), None);
        assert_eq!(name_country("Eurosport 1"), None);
        assert!(fits_country("Sky Sports UK", "uk") && fits_country("Sky Sports UK", "gb"));
        assert!(!fits_country("TF1 France", "be"));
        assert!(fits_country("Eurosport 1", "be"));
        let fold = |raw: &str| crate::live_tv::channels::normalize(&clean_name(raw, "ie"));
        assert_eq!(fold("RTE 1"), "rteone", "RTÉ One's identity (RTEOne.ie)");
        assert_eq!(fold("RTE 2"), "rte2", "RTÉ2's identity (RTE2.ie)");
        assert_eq!(category_for("beIN SPORTS 1"), Some("Sports"));
        assert_eq!(category_for("TF1"), None);
    }
}
