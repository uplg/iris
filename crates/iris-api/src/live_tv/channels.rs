//! Live TV channel model: turns raw playlist entries into a deduped,
//! TNT-ordered channel list with per-channel fallback sources.

use std::collections::HashMap;

use super::m3u::M3uEntry;

/// A playable channel. One channel aggregates every playlist entry that
/// resolves to the same identity (e.g. "Gulli" SD + HD, or the same channel
/// from two different playlists) as an ordered list of fallback sources.
#[derive(Debug, Clone)]
pub struct Channel {
    /// Stable slug used in URLs (`tf1`, `france-2`, …).
    pub id: String,
    /// Display name with quality/reliability suffixes stripped.
    pub name: String,
    /// Raw `tvg-id` of the first entry (kept for EPG matching).
    pub tvg_id: Option<String>,
    pub logo_url: Option<String>,
    /// `group-title` split on `;`.
    pub categories: Vec<String>,
    pub geo_blocked: bool,
    pub not_24_7: bool,
    /// French TNT channel number (Arcom numbering since 2025-06-06) when the
    /// channel is a national TNT network — drives the pinned section.
    pub tnt_number: Option<u16>,
    /// Upstream candidates, best quality first. Never exposed to clients.
    pub sources: Vec<StreamSource>,
}

/// One upstream stream URL plus the request headers it demands.
#[derive(Debug, Clone)]
pub struct StreamSource {
    pub url: String,
    /// Vertical resolution parsed from the entry name ("(1080p)" → 1080).
    pub quality: Option<u32>,
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    /// Reliability prior derived from the host (see [`SourceTier`]). Seeds
    /// the election order — the proxy prefers a stable origin and only rotates
    /// to a community restream when the officials fail.
    pub tier: SourceTier,
}

/// Stream-source reliability tiers, best first (lower sorts first). Derived
/// from the URL host: an official broadcaster CDN is the most stable, a known
/// ISP restream next, everything else (community aggregators, github-hosted
/// redirects) last. Sources are ordered by `(tier, quality)` so a healthy
/// official feed always outranks a community restream of the same channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceTier {
    Official,
    Isp,
    Community,
}

/// Host substrings identifying a tier — the ONE place source-reliability
/// priors live. First matching tier wins; order inside a tier is irrelevant.
/// Extend as new stable origins appear. A mis-classification is safe: it only
/// changes the try-order, never correctness (a wrongly-Community official is
/// tried a bit later, that's all).
const TIER_HOST_MARKERS: &[(SourceTier, &[&str])] = &[
    (
        SourceTier::Official,
        &[
            // FR broadcaster CDNs.
            "tf1.fr",
            "tf1.net",
            "france.tv",
            "francetv",
            "ftvcdn",
            "m6.fr",
            "6play",
            "6cloud",
            "arte.tv",
            "artecdn",
            "canalplus",
            "canal-plus",
            "mycanal",
            "bfmtv",
            "cnews",
            "lequipe.fr",
            "rmc",
            "lcp.fr",
            "publicsenat",
            "franceinfo",
            "radiofrance",
            "sfrpresse",
            // US: official FAST providers (licensed, ad-supported — the
            // stable backbone of free US TV) + their delivery partners.
            "pluto.tv",
            "tubi.video",
            "tubi.io",
            "amagi.tv",
            "getpublica.com",
            "samsungtvplus",
            "samsung-",
            "plex.tv",
            "provider.plex.tv",
            "roku.com",
            "therokuchannel",
            "telvue.com",
            "xumo",
            // IE/UK broadcaster + national-broadcast infra.
            "rte.ie",
            "rtecdn",
            "tibus.net",
            "sharp-stream",
        ],
    ),
    (
        SourceTier::Isp,
        &[
            // FR ISP TNT restreams.
            "netplus.ch",
            "free.fr",
            "proxad",
            "orange.fr",
            "sfr.fr",
            "bouyguestelecom",
        ],
    ),
];

/// Classify a stream URL into a reliability tier from its host. Unknown hosts
/// fall to [`SourceTier::Community`] (tried last, after all officials/ISPs).
pub fn classify_source(url: &str) -> SourceTier {
    let host = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    for (tier, markers) in TIER_HOST_MARKERS {
        if markers.iter().any(|m| host.contains(m)) {
            return *tier;
        }
    }
    SourceTier::Community
}

/// Election sort key: best tier first, then best quality (unknown quality
/// last). The single source-ordering rule, shared by every builder so the
/// pre-election order can't drift between them.
fn source_order_key(s: &StreamSource) -> (SourceTier, std::cmp::Reverse<u32>) {
    (s.tier, std::cmp::Reverse(s.quality.unwrap_or(0)))
}

/// Official TNT numbering (Arcom, effective 2025-06-06). Keys are normalized
/// aliases (lowercase alphanumeric) matched against the tvg-id base and the
/// cleaned display name — several aliases per channel because playlists
/// disagree on ids (`TFX` vs legacy `NT1`, `CStar` vs `D17`, …).
const TNT_CHANNELS: &[(u16, &[&str])] = &[
    (1, &["tf1"]),
    (2, &["france2"]),
    (3, &["france3"]),
    (4, &["france4"]),
    (5, &["france5"]),
    (6, &["m6"]),
    (7, &["arte"]),
    (
        8,
        &["lachaineparlementaire", "lcp", "lcpan", "lcppublicsenat"],
    ),
    (9, &["w9"]),
    (10, &["tmc"]),
    (11, &["tfx", "nt1"]),
    (12, &["gulli"]),
    (13, &["bfmtv"]),
    (14, &["cnews", "itele"]),
    (15, &["lci"]),
    (16, &["franceinfo"]),
    (17, &["cstar", "d17"]),
    (18, &["t18", "cmitv"]),
    (19, &["novo19", "oftv"]),
    (20, &["tf1seriesfilms", "hd1"]),
    (
        21,
        &["lequipe", "lequipetv", "equipetv", "equipe21", "lequipe21"],
    ),
    (22, &["6ter"]),
    (23, &["rmcstory", "numero23"]),
    (24, &["rmcdecouverte"]),
    (25, &["cherie25"]),
];

/// Build the channel list from one or more parsed playlists. Later playlists
/// merge into channels discovered by earlier ones (extra fallback sources)
/// rather than duplicating them. `tnt_overrides` is `Some` only for the
/// French list — the Arcom numbering table means nothing elsewhere.
// Config hands us a concrete std HashMap; generalizing the hasher here buys
// nothing for an internal fn.
#[allow(clippy::implicit_hasher)]
pub fn build_channels(
    playlists: &[Vec<M3uEntry>],
    tnt_overrides: Option<&HashMap<String, u16>>,
) -> Vec<Channel> {
    let mut channels: Vec<Channel> = Vec::new();
    // identity key → index into `channels`
    let mut by_identity: HashMap<String, usize> = HashMap::new();
    // TNT number → index: playlists disagree on tvg-ids for the same network
    // (`LEquipe.fr` vs `LEquipe21.fr`), which would otherwise yield two
    // channels both numbered N — one of them typically dead.
    let mut by_tnt: HashMap<u16, usize> = HashMap::new();

    for playlist in playlists {
        for entry in playlist {
            if entry.url.is_empty() {
                continue;
            }
            let (name, quality, geo_blocked, not_24_7) = clean_name(&entry.name);
            if name.is_empty() {
                continue;
            }
            let tvg_id = entry.attrs.get("tvg-id").filter(|s| !s.is_empty());
            let identity = tvg_id
                .map(|id| normalize(tvg_id_base(id)))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| normalize(&name));

            let source = StreamSource {
                tier: classify_source(&entry.url),
                url: entry.url.clone(),
                quality,
                user_agent: entry.header("http-user-agent").map(str::to_string),
                referrer: entry.header("http-referrer").map(str::to_string),
            };

            let tnt_number = tnt_number_for(&identity, &normalize(&name), tnt_overrides);
            let merge_idx = by_identity
                .get(&identity)
                .or_else(|| tnt_number.and_then(|n| by_tnt.get(&n)))
                .copied();
            if let Some(idx) = merge_idx {
                by_identity.entry(identity).or_insert(idx);
                let ch = &mut channels[idx];
                if ch.sources.iter().all(|s| s.url != source.url) {
                    ch.sources.push(source);
                }
                // Fill blanks the first entry lacked; flags are OR'd per
                // source so one reliable source clears nothing.
                if ch.logo_url.is_none() {
                    ch.logo_url = entry
                        .attrs
                        .get("tvg-logo")
                        .cloned()
                        .filter(|s| !s.is_empty());
                }
                ch.geo_blocked &= geo_blocked;
                ch.not_24_7 &= not_24_7;
                continue;
            }

            if let Some(n) = tnt_number {
                by_tnt.insert(n, channels.len());
            }
            by_identity.insert(identity.clone(), channels.len());
            channels.push(Channel {
                id: identity,
                name,
                tvg_id: tvg_id.cloned(),
                logo_url: entry
                    .attrs
                    .get("tvg-logo")
                    .cloned()
                    .filter(|s| !s.is_empty()),
                categories: entry
                    .attrs
                    .get("group-title")
                    .map(|g| {
                        g.split(';')
                            .map(str::trim)
                            .filter(|s| !s.is_empty() && *s != "Undefined")
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                geo_blocked,
                not_24_7,
                tnt_number,
                sources: vec![source],
            });
        }
    }

    for ch in &mut channels {
        ch.sources.sort_by_key(source_order_key);
    }

    // TNT channels first in Arcom order, then everything else grouped by
    // first category (alphabetical), alphabetical within a group,
    // uncategorized channels last.
    channels.sort_by(|a, b| {
        let key = |c: &Channel| {
            (
                c.tnt_number.unwrap_or(u16::MAX),
                c.categories
                    .first()
                    .cloned()
                    .map_or((1u8, String::new()), |g| (0, g)),
                c.name.to_lowercase(),
            )
        };
        key(a).cmp(&key(b))
    });

    channels
}

fn tnt_number_for(
    identity: &str,
    name_key: &str,
    overrides: Option<&HashMap<String, u16>>,
) -> Option<u16> {
    let overrides = overrides?;
    for (alias, num) in overrides {
        let alias = normalize(alias);
        if alias == identity || alias == name_key {
            return Some(*num);
        }
    }
    TNT_CHANNELS
        .iter()
        .find(|(_, aliases)| aliases.contains(&identity) || aliases.contains(&name_key))
        .map(|(num, _)| *num)
}

/// Key into iptv-org's stream database (`streams.json` `channel` field):
/// the full tvg-id without the `@variant` qualifier, lowercased —
/// `"M6.fr@HD"` → `"m6.fr"`. Country TLD kept: unlike [`tvg_id_base`] this
/// must not collide across countries.
pub fn db_key(tvg_id: &str) -> String {
    tvg_id.split('@').next().unwrap_or(tvg_id).to_lowercase()
}

/// Merge extra feeds from iptv-org's stream database into already-built
/// channels: every database URL not present yet becomes an additional
/// fallback source. The per-country playlist embeds only ONE feed per
/// channel — the database is where the alternates live (e.g. M6 has a dead
/// 1080p feed in the playlist and a working 720p one only in the database).
#[allow(clippy::implicit_hasher)]
pub fn merge_db_sources(channels: &mut [Channel], db: &HashMap<String, Vec<StreamSource>>) {
    for ch in channels.iter_mut() {
        let Some(tvg_id) = ch.tvg_id.as_ref() else {
            continue;
        };
        let Some(extra) = db.get(&db_key(tvg_id)) else {
            continue;
        };
        for source in extra {
            if ch.sources.iter().all(|s| s.url != source.url) {
                ch.sources.push(source.clone());
            }
        }
        ch.sources.sort_by_key(source_order_key);
    }
}

/// `"1080p"` → `Some(1080)` (iptv-org database `quality` field).
pub fn parse_quality(q: &str) -> Option<u32> {
    q.trim().strip_suffix('p').and_then(|n| n.parse().ok())
}

/// `"TF1SeriesFilms.fr@HD"` → `"TF1SeriesFilms"`: strip the `@variant`
/// qualifier then a trailing 2-letter country TLD.
pub(crate) fn tvg_id_base(tvg_id: &str) -> &str {
    let base = tvg_id.split('@').next().unwrap_or(tvg_id);
    match base.rsplit_once('.') {
        Some((head, tld)) if tld.len() == 2 && tld.chars().all(|c| c.is_ascii_alphabetic()) => head,
        _ => base,
    }
}

/// Lowercase alphanumeric fold: `"L'Équipe TV"` → `"lequipetv"`.
pub(crate) fn normalize(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            let c = match c {
                'à' | 'â' | 'ä' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'î' | 'ï' => 'i',
                'ô' | 'ö' => 'o',
                'ù' | 'û' | 'ü' => 'u',
                'ç' => 'c',
                _ => c,
            };
            c.is_ascii_alphanumeric().then(|| c.to_ascii_lowercase())
        })
        .collect()
}

/// Strip iptv-org name decorations: `"TF1 (720p) [Geo-blocked] [Not 24/7]"`
/// → `("TF1", Some(720), true, true)`.
fn clean_name(raw: &str) -> (String, Option<u32>, bool, bool) {
    let mut name = raw.trim().to_string();
    let mut quality = None;
    let mut geo_blocked = false;
    let mut not_24_7 = false;

    loop {
        let trimmed = name.trim_end().to_string();
        if let Some(start) = trimmed.rfind('[')
            && trimmed.ends_with(']')
        {
            let marker = &trimmed[start + 1..trimmed.len() - 1];
            if marker.eq_ignore_ascii_case("geo-blocked") {
                geo_blocked = true;
            } else if marker.eq_ignore_ascii_case("not 24/7") {
                not_24_7 = true;
            }
            name = trimmed[..start].to_string();
            continue;
        }
        if let Some(start) = trimmed.rfind('(')
            && trimmed.ends_with(')')
        {
            let inner = &trimmed[start + 1..trimmed.len() - 1];
            if let Some(q) = inner.strip_suffix('p').and_then(|n| n.parse::<u32>().ok()) {
                quality = Some(q);
                name = trimmed[..start].to_string();
                continue;
            }
        }
        name = trimmed;
        break;
    }
    // Channel-number prefixes ("10. TMC") are display artifacts of curated
    // lists; strip them so the identity folds to the bare network name.
    let name = name.trim();
    let name = name
        .split_once(". ")
        .filter(|(n, rest)| {
            !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && !rest.trim().is_empty()
        })
        .map_or(name, |(_, rest)| rest);
    (name.trim().to_string(), quality, geo_blocked, not_24_7)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tvg_id: &str, name: &str, url: &str, group: &str) -> M3uEntry {
        let mut attrs = HashMap::new();
        if !tvg_id.is_empty() {
            attrs.insert("tvg-id".to_string(), tvg_id.to_string());
        }
        if !group.is_empty() {
            attrs.insert("group-title".to_string(), group.to_string());
        }
        M3uEntry {
            name: name.to_string(),
            attrs,
            vlc_opts: HashMap::new(),
            url: url.to_string(),
        }
    }

    #[test]
    fn clean_name_strips_quality_and_markers() {
        assert_eq!(
            clean_name("Alpe d’Huez TV (720p) [Not 24/7]"),
            ("Alpe d’Huez TV".to_string(), Some(720), false, true)
        );
        assert_eq!(
            clean_name("Arte (720p) [Geo-blocked]"),
            ("Arte".to_string(), Some(720), true, false)
        );
        // Parenthesised text that is not a quality stays part of the name.
        assert_eq!(clean_name("Canal 32 (Troyes)").0, "Canal 32 (Troyes)");
        // Curated-list channel-number prefixes and provenance brackets
        // (ParaTV style) fold away; a name that merely contains digits+dot
        // mid-word does not.
        assert_eq!(clean_name("10. TMC [720p-tf1.fr]").0, "TMC");
        assert_eq!(clean_name("46. ARTE [1080p-tf1.fr]").0, "ARTE");
        assert_eq!(clean_name("Ciné 2. Le retour").0, "Ciné 2. Le retour");
    }

    #[test]
    fn tnt_number_unifies_divergent_tvg_ids() {
        // Playlists disagree on the network's tvg-id ("LEquipe.fr" vs
        // "LEquipe21.fr") — both map to TNT 21 and must land in ONE channel,
        // not two rows both numbered 21 with one of them dead.
        let playlists = vec![
            vec![entry(
                "LEquipe.fr",
                "L'Équipe (720p)",
                "http://a/equipe.m3u8",
                "General",
            )],
            vec![entry(
                "LEquipe21.fr",
                "L'ÉQUIPE",
                "http://b/lequipe.m3u8",
                "General",
            )],
        ];
        let channels = build_channels(&playlists, Some(&HashMap::new()));
        let equipe: Vec<_> = channels.iter().filter(|c| c.tnt_number == Some(21)).collect();
        assert_eq!(equipe.len(), 1);
        assert_eq!(equipe[0].sources.len(), 2);
        // Without TNT overrides (non-FR countries) the ids stay distinct.
        let separate = build_channels(&playlists, None);
        assert_eq!(separate.len(), 2);
    }

    #[test]
    fn tvg_id_base_strips_variant_and_country() {
        assert_eq!(tvg_id_base("TF1.fr@SD"), "TF1");
        assert_eq!(tvg_id_base("BabyTV.uk@FranceHD"), "BabyTV");
        assert_eq!(tvg_id_base("France3.fr@National"), "France3");
        assert_eq!(tvg_id_base("NoCountry"), "NoCountry");
    }

    #[test]
    fn tnt_channels_pinned_in_arcom_order_then_categories() {
        let playlists = vec![vec![
            entry("Zebra.fr", "Zebra TV", "http://x/zebra", "General"),
            entry("M6.fr@HD", "M6 (1080p)", "http://x/m6", "Entertainment"),
            entry("TF1.fr@SD", "TF1 (720p)", "http://x/tf1", "Entertainment"),
            entry("Aardvark.fr", "Aardvark", "http://x/aard", "General"),
            entry("", "Mystery Channel", "http://x/mystery", ""),
        ]];
        let channels = build_channels(&playlists, Some(&HashMap::new()));
        let names: Vec<&str> = channels.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["TF1", "M6", "Aardvark", "Zebra TV", "Mystery Channel"]
        );
        assert_eq!(channels[0].tnt_number, Some(1));
        assert_eq!(channels[1].tnt_number, Some(6));
        assert_eq!(channels[4].tnt_number, None);
    }

    #[test]
    fn same_identity_merges_sources_best_quality_first() {
        let playlists = vec![
            vec![
                entry("Gulli.fr@SD", "Gulli", "http://x/gulli-sd", "Kids"),
                entry(
                    "Gulli.fr@HD",
                    "Gulli HD (720p)",
                    "http://x/gulli-hd",
                    "Kids",
                ),
            ],
            // second playlist: same channel via name match → extra fallback
            vec![entry("", "Gulli (1080p)", "http://y/gulli", "Kids")],
        ];
        let channels = build_channels(&playlists, Some(&HashMap::new()));
        assert_eq!(channels.len(), 1);
        let ch = &channels[0];
        assert_eq!(ch.tnt_number, Some(12));
        let urls: Vec<&str> = ch.sources.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["http://y/gulli", "http://x/gulli-hd", "http://x/gulli-sd"]
        );
        // duplicate URL is not re-added
        let playlists2 = vec![playlists[0].clone(), playlists[0].clone()];
        assert_eq!(
            build_channels(&playlists2, Some(&HashMap::new()))[0]
                .sources
                .len(),
            2
        );
    }

    #[test]
    fn overrides_win_over_builtin_table() {
        let playlists = vec![vec![entry(
            "CanalPlus.fr@SD",
            "Canal+ (1080p)",
            "http://x/cplus",
            "Entertainment",
        )]];
        let mut overrides = HashMap::new();
        overrides.insert("CanalPlus".to_string(), 4u16);
        let channels = build_channels(&playlists, Some(&overrides));
        assert_eq!(channels[0].tnt_number, Some(4));
        assert_eq!(
            build_channels(&playlists, Some(&HashMap::new()))[0].tnt_number,
            None
        );
    }

    #[test]
    fn db_merge_adds_alternate_feeds_without_duplicates() {
        let playlists = vec![vec![entry(
            "M6.fr@HD",
            "M6 (1080p)",
            "http://dead/M6.m3u8",
            "Entertainment",
        )]];
        let mut channels = build_channels(&playlists, Some(&HashMap::new()));
        let mut db: HashMap<String, Vec<StreamSource>> = HashMap::new();
        db.insert(
            "m6.fr".to_string(),
            vec![
                StreamSource {
                    // already known from the playlist → must not duplicate
                    url: "http://dead/M6.m3u8".to_string(),
                    quality: Some(1080),
                    user_agent: None,
                    referrer: None,
                    tier: SourceTier::Community,
                },
                StreamSource {
                    url: "http://alt/M6-HD/index.m3u8".to_string(),
                    quality: Some(720),
                    user_agent: None,
                    referrer: None,
                    tier: SourceTier::Community,
                },
            ],
        );
        merge_db_sources(&mut channels, &db);
        let urls: Vec<&str> = channels[0].sources.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["http://dead/M6.m3u8", "http://alt/M6-HD/index.m3u8"]
        );
        // channel without tvg-id or without db entry is untouched
        let playlists2 = vec![vec![entry("", "Mystery", "http://x/mys.m3u8", "")]];
        let mut channels2 = build_channels(&playlists2, None);
        merge_db_sources(&mut channels2, &db);
        assert_eq!(channels2[0].sources.len(), 1);
    }

    #[test]
    fn classify_source_by_host() {
        use SourceTier::{Community, Isp, Official};
        assert_eq!(
            classify_source("https://live-tmc-hls.cdn-0.diff.tf1.fr/x.m3u8"),
            Official
        );
        assert_eq!(
            classify_source("https://mabusetv.francetv.fr/hls/y.m3u8"),
            Official
        );
        assert_eq!(
            classify_source("https://viamotionhsi.netplus.ch/live/z/index.m3u8"),
            Isp
        );
        // US FAST providers + IE broadcaster infra rank as official/stable.
        assert_eq!(
            classify_source("https://service-stitcher.clusters.pluto.tv/v2/x.m3u8"),
            Official
        );
        assert_eq!(
            classify_source("https://cdn-uw2-prod.tsv2.amagi.tv/y/playlist.m3u8"),
            Official
        );
        assert_eq!(
            classify_source("https://aegis-cloudfront-1.tubi.video/z.m3u8"),
            Official
        );
        assert_eq!(
            classify_source("https://something.rte.ie/live/a.m3u8"),
            Official
        );
        assert_eq!(
            classify_source("http://user:pw@random-cdn.example.net:8080/a.m3u8"),
            Community
        );
        assert_eq!(
            classify_source("https://raw.githubusercontent.com/x/y/fr.m3u8"),
            Community
        );
    }

    #[test]
    fn official_source_outranks_community_regardless_of_quality() {
        // A community 1080p must NOT beat an official 720p: stability first.
        let playlists = vec![
            vec![entry(
                "TF1.fr",
                "TF1 (1080p)",
                "http://randomcdn.example/tf1",
                "General",
            )],
            vec![entry(
                "TF1.fr",
                "TF1 (720p)",
                "https://live.tf1.fr/tf1/index.m3u8",
                "General",
            )],
        ];
        let ch = &build_channels(&playlists, Some(&HashMap::new()))[0];
        let urls: Vec<&str> = ch.sources.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://live.tf1.fr/tf1/index.m3u8",
                "http://randomcdn.example/tf1"
            ]
        );
        assert_eq!(ch.sources[0].tier, SourceTier::Official);
    }

    #[test]
    fn db_key_and_quality_parsing() {
        assert_eq!(db_key("M6.fr@HD"), "m6.fr");
        assert_eq!(db_key("Gulli.fr"), "gulli.fr");
        assert_eq!(parse_quality("1080p"), Some(1080));
        assert_eq!(parse_quality("480i"), None);
        assert_eq!(parse_quality(""), None);
    }

    #[test]
    fn no_tnt_pinning_outside_france() {
        let playlists = vec![vec![entry("TF1.fr@SD", "TF1", "http://x/tf1", "General")]];
        assert_eq!(build_channels(&playlists, None)[0].tnt_number, None);
    }

    #[test]
    fn legacy_alias_matches_tfx() {
        let playlists = vec![vec![entry("NT1.fr", "TFX", "http://x/tfx", "Series")]];
        assert_eq!(
            build_channels(&playlists, Some(&HashMap::new()))[0].tnt_number,
            Some(11)
        );
    }
}
