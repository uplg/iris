use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicUsize};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::*;
use crate::live_tv::channels::{Channel, StreamSource, classify_source};
use crate::live_tv::{LiveTvService, ProxiedResponse};

#[test]
fn sentinel_round_trips() {
    assert_eq!(sentinel(469, 2), "dlive://469/2");
    assert_eq!(parse_sentinel("dlive://469/2"), Some((469, 2)));
    assert_eq!(parse_sentinel("dlive://469"), None);
    assert_eq!(parse_sentinel("vavoo://469/2"), None);
}

#[test]
fn breaker_opens_on_a_run_then_lets_one_probe_through() {
    let t0 = Instant::now();
    let mut b = Breaker::new(3, Duration::from_mins(15));
    b.failure(t0);
    b.failure(t0);
    b.success();
    b.failure(t0);
    b.failure(t0);
    assert!(b.admits(t0), "a success resets the run");
    b.failure(t0);
    assert!(b.is_open(t0));
    assert!(!b.admits(t0 + Duration::from_mins(14)));
    let later = t0 + Duration::from_mins(15);
    assert!(b.admits(later), "one half-open probe");
    assert!(!b.admits(later), "only one at a time");
    assert!(
        b.admits(later + PROBE_STALE),
        "a probe that never reported stops blocking"
    );
    b.failure(later + PROBE_STALE);
    assert!(b.is_open(later + PROBE_STALE), "a failed probe re-opens");
    let reopened = later + PROBE_STALE + Duration::from_mins(15);
    assert!(b.admits(reopened));
    b.success();
    assert!(b.admits(reopened) && b.admits(reopened), "closed again");
}

#[test]
fn breaker_trip_opens_at_once() {
    let t0 = Instant::now();
    let mut b = Breaker::new(4, Duration::from_mins(10));
    b.trip(t0);
    assert!(b.is_open(t0) && !b.admits(t0 + Duration::from_mins(9)));
}

#[test]
fn page_budget_is_a_sliding_window() {
    let t0 = Instant::now();
    let mut pages = PageBudget {
        budget: 20,
        window: Duration::from_mins(10),
        loads: VecDeque::new(),
    };
    for i in 0..20 {
        assert!(pages.take(t0 + Duration::from_secs(i)), "load {i}");
    }
    assert!(
        !pages.take(t0 + Duration::from_mins(5)),
        "21st in the window"
    );
    assert!(!pages.take(t0 + Duration::from_secs(599)));
    assert!(
        pages.take(t0 + Duration::from_secs(600)),
        "the first expired"
    );
    assert!(!pages.take(t0 + Duration::from_secs(600)));
    assert_eq!(pages.used(t0 + Duration::from_mins(30)), 0);
}

#[test]
fn entries_group_per_player_with_folded_names() {
    let index = Index {
        names: HashMap::from([
            (469, "TF1 France".to_string()),
            (116, "beIN SPORTS 1 France".to_string()),
        ]),
    };
    let lists = entries_for(&index, &[469, 116, 999], &[1, 2, 9, 2, 6], "fr");
    let origins: Vec<SourceOrigin> = lists.iter().map(|(o, _)| *o).collect();
    assert_eq!(
        origins,
        vec![
            SourceOrigin::Dlive { rank: 0 },
            SourceOrigin::Dlive { rank: 1 },
            SourceOrigin::Dlive { rank: 2 },
        ],
        "unknown and repeated players dropped"
    );
    let p1 = &lists[0].1;
    assert_eq!(p1.len(), 2, "an id missing from the list is skipped");
    assert_eq!(p1[0].name, "TF1");
    assert_eq!(p1[0].url, "dlive://469/1");
    assert_eq!(p1[1].name, "beIN SPORTS 1");
    assert_eq!(
        p1[1].attrs.get("group-title").map(String::as_str),
        Some("Sports")
    );
    assert_eq!(lists[2].1[0].url, "dlive://469/6");
}

#[test]
fn dlive_entries_merge_into_iptv_org_channels_or_stand_alone() {
    let index = Index {
        names: HashMap::from([
            (469, "TF1 France".to_string()),
            (645, "L'Equipe France".to_string()),
            (116, "beIN SPORTS 1 France".to_string()),
        ]),
    };
    let iptv = |tvg: &str, name: &str, url: &str| M3uEntry {
        name: name.to_string(),
        attrs: HashMap::from([("tvg-id".to_string(), tvg.to_string())]),
        vlc_opts: HashMap::new(),
        kodi_props: HashMap::new(),
        url: url.to_string(),
    };
    let mut lists = vec![(
        SourceOrigin::IptvOrg,
        vec![
            iptv("TF1.fr@SD", "TF1 (720p)", "http://x/tf1"),
            iptv("LEquipe21.fr", "L'Équipe 21", "http://x/equipe"),
        ],
    )];
    lists.extend(entries_for(&index, &[469, 645, 116], &[1, 2], "fr"));
    let channels = crate::live_tv::channels::build_channels(&lists, Some(&HashMap::new()));
    assert_eq!(channels.len(), 3);
    let tf1 = channels.iter().find(|c| c.id == "tf1").unwrap();
    let urls: Vec<&str> = tf1.sources.iter().map(|s| s.url.as_str()).collect();
    assert_eq!(urls, vec!["dlive://469/1", "dlive://469/2", "http://x/tf1"]);
    let equipe = channels.iter().find(|c| c.tnt_number == Some(21)).unwrap();
    assert_eq!(equipe.sources.len(), 3, "merged through the TNT aliases");
    let bein = channels.iter().find(|c| c.name == "beIN SPORTS 1").unwrap();
    assert_eq!(bein.sources.len(), 2, "a dlive-only channel");
    assert_eq!(bein.categories, vec!["Sports".to_string()]);
}

#[test]
fn rte_one_and_rte2_merge_with_free_tv() {
    let index = Index {
        names: HashMap::from([(364, "RTE 1".to_string()), (365, "RTE 2".to_string())]),
    };
    let free_tv = |tvg: &str, name: &str, url: &str| M3uEntry {
        name: name.to_string(),
        attrs: HashMap::from([("tvg-id".to_string(), tvg.to_string())]),
        vlc_opts: HashMap::new(),
        kodi_props: HashMap::new(),
        url: url.to_string(),
    };
    let mut lists = vec![(
        SourceOrigin::Extra,
        vec![
            free_tv("RTEOne.ie", "RTÉ One", "https://live.rte.ie/channel1.m3u8"),
            free_tv("RTE2.ie", "RTÉ2", "https://live.rte.ie/channel2.m3u8"),
        ],
    )];
    lists.extend(entries_for(&index, &[364, 365], &[1], "ie"));
    let channels = crate::live_tv::channels::build_channels(&lists, None);
    assert_eq!(channels.len(), 2);
    let one = channels.iter().find(|c| c.id == "rteone").unwrap();
    assert_eq!(one.name, "RTÉ One");
    assert_eq!(
        one.sources[0].url, "dlive://364/1",
        "dlive ahead of the locked feed"
    );
    let two = channels.iter().find(|c| c.id == "rte2").unwrap();
    assert_eq!(two.sources.len(), 2);
}

fn token_for(slug: &str, expiry: i64) -> String {
    format!(
        "{}.{}",
        STANDARD.encode(format!("{slug}|no_check_ip|{expiry}")),
        "ab".repeat(32)
    )
}

#[test]
fn current_form_swaps_stale_signatures_and_tokens_only() {
    let signed = Resolved {
        url: "https://e.example:8443/hls/a.m3u8?s=new&e=2".into(),
        user_agent: None,
        referrer: None,
        origin: None,
        kind: Kind::Signed,
        valid_until: i64::MAX,
    };
    let held = Url::parse("https://e.example:8443/hls/a.m3u8?s=old&e=1").unwrap();
    assert_eq!(
        current_form(&signed, &held).unwrap().as_str(),
        "https://e.example:8443/hls/a.m3u8?s=new&e=2"
    );
    let segment = Url::parse("https://e.example:8443/hls/a-1.ts").unwrap();
    assert_eq!(current_form(&signed, &segment), None);

    let token = Resolved {
        url: "https://ds.example/France2/index.m3u8?token=NEW".into(),
        user_agent: None,
        referrer: None,
        origin: None,
        kind: Kind::Token {
            slug: "France2".into(),
            token: "NEW".into(),
            refresh_url: String::new(),
        },
        valid_until: i64::MAX,
    };
    let variant = Url::parse("https://ds.example/France2/tracks-v1a1/mono.m3u8?token=OLD").unwrap();
    assert_eq!(
        current_form(&token, &variant).unwrap().as_str(),
        "https://ds.example/France2/tracks-v1a1/mono.m3u8?token=NEW"
    );
    let other_host = Url::parse("https://cdn.example/x.ts?token=OLD").unwrap();
    assert_eq!(current_form(&token, &other_host), None);
    let tiktok = Url::parse("https://ds.example/x.image?refresh_token=OLD").unwrap();
    assert_eq!(current_form(&token, &tiktok), None);
}

#[test]
fn recorded_playlists_rewrite_and_refresh_as_expected() {
    let signer = crate::live_tv::proxy::Signer::new("s");
    let proxied = |body: &str, base: &str| {
        let base = Url::parse(base).unwrap();
        crate::live_tv::proxy::rewrite_playlist(body, &base, "fr:tf1", None, &signer)
    };
    let p1 = include_str!("fixtures/p1.m3u8");
    assert_eq!(
        crate::live_tv::first_variant_uri(p1),
        None,
        "a media playlist"
    );
    let seg = Url::parse(p1.lines().find(|l| l.starts_with("https://p16")).unwrap()).unwrap();
    let out = proxied(p1, "https://edge.example/premium950/index.m3u8");
    assert!(out.contains(&crate::live_tv::proxy::proxied_url(
        "fr:tf1", None, &seg, &signer
    )));

    // Player 2 segments are relative and must lose the playlist's s/e.
    let p2_url = "https://e8975o.example:8443/hls/pq1gxvz9ymj74.m3u8?s=VG6&e=1791327573";
    let seg = Url::parse("https://e8975o.example:8443/hls/pq1gxvz9ymj74-1751469570.ts").unwrap();
    let out = proxied(include_str!("fixtures/p2.m3u8"), p2_url);
    assert!(out.contains(&crate::live_tv::proxy::proxied_url(
        "fr:tf1", None, &seg, &signer
    )));

    // Player 6: every URI carries the token; a renewed one is swapped in.
    let master = include_str!("fixtures/p6-master.m3u8");
    let base = Url::parse("https://ds167.example/France2/index.m3u8?token=OLD").unwrap();
    let variant = base
        .join(crate::live_tv::first_variant_uri(master).unwrap())
        .unwrap();
    let old = parse::query_param(variant.as_str(), "token")
        .unwrap()
        .to_string();
    let renewed = Resolved {
        url: "https://ds167.example/France2/index.m3u8?token=NEW".into(),
        user_agent: None,
        referrer: None,
        origin: None,
        kind: Kind::Token {
            slug: "France2".into(),
            token: "NEW".into(),
            refresh_url: String::new(),
        },
        valid_until: i64::MAX,
    };
    let current = current_form(&renewed, &variant).unwrap();
    assert_eq!(
        current.as_str(),
        variant.as_str().replace(&old, "NEW"),
        "only the token changes"
    );
    let media = include_str!("fixtures/p6-variant.m3u8");
    let first_segment = media.lines().find(|l| l.ends_with(&old)).unwrap();
    let segment = variant.join(first_segment).unwrap();
    assert!(
        current_form(&renewed, &segment)
            .unwrap()
            .as_str()
            .ends_with("token=NEW")
    );
}

/// A loopback stand-in for dlive.sx, its embeds and its edges.
#[derive(Default)]
struct Fake {
    site_pages: AtomicUsize,
    embed_pages: AtomicUsize,
    edge_hits: AtomicUsize,
    /// Player 1 playlist status (0 = 200).
    edge_status: AtomicU16,
    /// 0 = a real wrapped TS, 1 = a plain picture.
    segment_mode: AtomicU8,
    /// 0 = latest signature valid, 1 = the first one rejected, 2 = all.
    sig_mode: AtomicU8,
    refreshes: AtomicUsize,
    /// Player 1 the way it is since 2026-10-18: the playlist wants a token
    /// from the edge's API and the embed's `Origin`.
    edge_wants_token: AtomicBool,
    /// Player 1 embed pages read (edge learning).
    dembed_pages: AtomicUsize,
    /// Edge tokens handed out (`tok1`, `tok2`…), and the last one revoked.
    edge_tokens: AtomicUsize,
    edge_revoked: AtomicUsize,
}

impl Fake {
    /// Whether a request carries the embed page's `Origin`, as the real
    /// edge wants (`Referer` doesn't count).
    fn from_embed(headers: &HeaderMap) -> bool {
        headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
            == Some(&format!("http://{}", host(headers)))
    }

    fn edge_token_valid(&self, t: Option<&String>) -> bool {
        t.and_then(|t| t.strip_prefix("tok")?.parse::<usize>().ok())
            .is_some_and(|n| {
                n > self.edge_revoked.load(Ordering::SeqCst)
                    && n <= self.edge_tokens.load(Ordering::SeqCst)
            })
    }
}

type Shared = Arc<Fake>;

fn host(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

fn hls(body: String) -> Response {
    (
        [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body,
    )
        .into_response()
}

/// Encode a config the way the embeds do (the inverse of `parse::econfig`).
fn econfig_page(stream_url: &str) -> String {
    let mut json = serde_json::json!({ "stream_url": stream_url, "p2p": true }).to_string();
    while !json.len().is_multiple_of(9) {
        json.push(' ');
    }
    let inner = STANDARD.encode(&json);
    let m = inner.len() / 4;
    let parts: Vec<&str> = (0..4).map(|i| &inner[i * m..(i + 1) * m]).collect();
    let mut outer = String::new();
    for pos in [2usize, 0, 3, 1] {
        let mut chunk = STANDARD.encode(parts[pos]);
        chunk.insert(3, 'X');
        outer.push_str(&chunk);
    }
    format!(
        "<html><script>window._econfig='{}';</script></html>",
        STANDARD.encode(outer)
    )
}

async fn fake_site_page(
    State(fake): State<Shared>,
    headers: HeaderMap,
    Path((player, file)): Path<(String, String)>,
) -> Response {
    fake.site_pages.fetch_add(1, Ordering::SeqCst);
    let id = file.trim_start_matches("stream-").trim_end_matches(".php");
    let src = match player.as_str() {
        "cast" => format!("http://{}/e/code{id}", host(&headers)),
        "stream" => format!("http://{}/dembed?id={id}", host(&headers)),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    format!("<html><iframe src=\"{src}\"></iframe></html>").into_response()
}

async fn fake_embed(State(fake): State<Shared>, headers: HeaderMap) -> Response {
    if headers.get(header::REFERER).is_none() {
        return "<h4>Not allowed</h4><p>This stream is domain protected</p>".into_response();
    }
    let n = fake.embed_pages.fetch_add(1, Ordering::SeqCst) + 1;
    let expiry = epoch_s() + 6 * 3600;
    econfig_page(&format!(
        "http://{}/hls/x.m3u8?s=sig{n}&e={expiry}",
        host(&headers)
    ))
    .into_response()
}

async fn fake_signed(
    State(fake): State<Shared>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let latest = format!("sig{}", fake.embed_pages.load(Ordering::SeqCst));
    let sig = q.get("s").cloned().unwrap_or_default();
    let valid = match fake.sig_mode.load(Ordering::SeqCst) {
        0 => sig == latest,
        1 => sig == latest && sig != "sig1",
        _ => false,
    };
    let ua_ok = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        == Some(UA);
    if !(valid && ua_ok) {
        return StatusCode::FORBIDDEN.into_response();
    }
    hls("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nx-1.ts\n".into())
}

async fn fake_edge(
    State(fake): State<Shared>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    fake.edge_hits.fetch_add(1, Ordering::SeqCst);
    let status = fake.edge_status.load(Ordering::SeqCst);
    if status != 0 {
        return StatusCode::from_u16(status).unwrap().into_response();
    }
    if fake.edge_wants_token.load(Ordering::SeqCst)
        && !(fake.edge_token_valid(q.get("t")) && Fake::from_embed(&headers))
    {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    hls(format!(
        "#EXTM3U\n#EXT-X-TARGETDURATION:7\n#EXTINF:6.0,\nhttp://{}/img/a.image?refresh_token=x\n",
        host(&headers)
    ))
}

async fn fake_image(State(fake): State<Shared>) -> Response {
    let body = if fake.segment_mode.load(Ordering::SeqCst) == 0 {
        unwrap::tests::pixel_png(&unwrap::tests::ts())
    } else {
        unwrap::tests::png(b"a holiday picture", 2, &[])
    };
    ([(header::CONTENT_TYPE, "image/png")], body).into_response()
}

/// Player 1's embed: the playlist of the id it is asked about, and with
/// `edge_wants_token` the token call of the real one
/// (`fixtures/dembed-51.html`).
async fn fake_dembed(
    State(fake): State<Shared>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    fake.dembed_pages.fetch_add(1, Ordering::SeqCst);
    let id = q.get("id").cloned().unwrap_or_default();
    let host = host(&headers);
    let token = if fake.edge_wants_token.load(Ordering::SeqCst) {
        format!(
            "const CHANNEL = \"premium{id}\";\nconst EDGE = \"http://{host}/edge\";\n\
             fetch(EDGE + '/api/playlist-token?slug=' + encodeURIComponent(CHANNEL), {{ cache: 'no-store' }})\n"
        )
    } else {
        String::new()
    };
    format!("<script>const SRC = \"http://{host}/edge/premium{id}/index.m3u8\";\n{token}</script>")
        .into_response()
}

/// Player 1's token endpoint: only for the embed's `Origin`.
async fn fake_edge_token(State(fake): State<Shared>, headers: HeaderMap) -> Response {
    if !Fake::from_embed(&headers) {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    let n = fake.edge_tokens.fetch_add(1, Ordering::SeqCst) + 1;
    axum::Json(serde_json::json!({ "token": format!("tok{n}"), "ttl": 120 })).into_response()
}

async fn fake_vavoo() -> Response {
    hls("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nv-1.ts\n".into())
}

async fn fake_refresh(State(fake): State<Shared>) -> Response {
    fake.refreshes.fetch_add(1, Ordering::SeqCst);
    axum::Json(serde_json::json!({
        "success": true,
        "token": token_for("France2", epoch_s() + 300),
        "expires_in": 300,
    }))
    .into_response()
}

async fn serve(fake: Shared) -> std::net::SocketAddr {
    let app = axum::Router::new()
        .route("/{player}/{file}", get(fake_site_page))
        .route("/e/{code}", get(fake_embed))
        .route("/hls/x.m3u8", get(fake_signed))
        .route("/edge/{premium}/index.m3u8", get(fake_edge))
        .route("/edge/api/playlist-token", get(fake_edge_token))
        .route("/dembed", get(fake_dembed))
        .route("/img/{name}", get(fake_image))
        .route("/vavoo.m3u8", get(fake_vavoo))
        .route("/api/refresh_token.php", post(fake_refresh))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

fn config(base_url: String) -> iris_config::LiveTvConfig {
    let mut cfg = iris_config::LiveTvConfig::default();
    cfg.dlive.enabled = true;
    cfg.dlive.base_url = base_url;
    cfg.dlive.countries = HashMap::from([("fr".to_string(), vec![469])]);
    cfg
}

struct Rig {
    fake: Shared,
    addr: std::net::SocketAddr,
    svc: LiveTvService,
}

impl Rig {
    async fn new() -> Self {
        Self::with(|_| {}).await
    }

    async fn with(tweak: impl FnOnce(&mut iris_config::LiveTvConfig)) -> Self {
        let fake = Shared::default();
        let addr = serve(fake.clone()).await;
        let mut cfg = config(format!("http://{addr}"));
        tweak(&mut cfg);
        let svc = LiveTvService::new(cfg, "test-secret").unwrap();
        *svc.inner.dlive.edge.write().unwrap() = Some(Edge {
            template: format!("http://{addr}/edge/premium{{id}}/index.m3u8"),
            embed_url: format!("http://{addr}/dembed?id=469"),
            token: None,
            origin: None,
        });
        Self { fake, addr, svc }
    }

    fn dlive(&self) -> &Arc<Dlive> {
        &self.svc.inner.dlive
    }

    /// Player 1 as it is since 2026-10-18: the edge wants a token asked with
    /// the embed's `Origin`, learned that way.
    fn tokened(&self) {
        self.fake.edge_wants_token.store(true, Ordering::SeqCst);
        let mut edge = self.dlive().edge.write().unwrap();
        let edge = edge.as_mut().unwrap();
        edge.token = Some(format!(
            "http://{}/edge/api/playlist-token?slug=premium{{id}}",
            self.addr
        ));
        edge.origin = Some(format!("http://{}", self.addr));
    }

    /// Wait for a background edge re-learning to land.
    async fn edge_learned(&self) {
        for _ in 0..200 {
            if self.dlive().edge_generation() > 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the edge was never learned");
    }

    fn vavoo(&self) -> String {
        format!("http://{}/vavoo.m3u8", self.addr)
    }

    fn cache_embed(&self, id: u32, player: u8) {
        self.dlive().embeds.write().unwrap().insert(
            (id, player),
            (
                Embed::Found(format!("http://{}/e/code{id}", self.addr)),
                super::epoch_s(),
            ),
        );
    }

    /// Install a one-channel `fr` snapshot with these sources, in order.
    fn channel(&self, sources: &[(&str, SourceOrigin)]) -> Arc<crate::live_tv::CountrySnapshot> {
        let ch = Channel {
            id: "tf1".into(),
            legacy_ids: Vec::new(),
            name: "TF1".into(),
            tvg_id: None,
            logo_url: None,
            categories: Vec::new(),
            geo_blocked: false,
            not_24_7: false,
            tnt_number: None,
            sources: sources
                .iter()
                .map(|(url, origin)| StreamSource {
                    url: (*url).to_string(),
                    quality: None,
                    user_agent: None,
                    referrer: None,
                    tier: classify_source(url),
                    origin: *origin,
                    licence: None,
                })
                .collect(),
        };
        let snap = Arc::new(self.svc.build_snapshot(vec![ch]));
        self.svc
            .inner
            .snapshots
            .write()
            .unwrap()
            .insert("fr".into(), snap.clone());
        snap
    }
}

fn hits(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::SeqCst)
}

const P1: SourceOrigin = SourceOrigin::Dlive { rank: 0 };
const P2: SourceOrigin = SourceOrigin::Dlive { rank: 1 };

fn cooling(snap: &crate::live_tv::CountrySnapshot, si: usize) -> bool {
    snap.health[0][si].in_cooldown(crate::live_tv::epoch_ms())
}

#[tokio::test]
async fn a_channel_missing_on_player_1_falls_to_vavoo() {
    let rig = Rig::new().await;
    rig.fake.edge_status.store(404, Ordering::SeqCst);
    let vavoo = rig.vavoo();
    let snap = rig.channel(&[("dlive://469/1", P1), (&vavoo, SourceOrigin::Vavoo)]);
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 1);
    assert!(cooling(&snap, 0), "the missing feed cools down");
    assert!(!rig.dlive().breaker_open(), "a 404 is not an outage");
}

#[tokio::test]
async fn an_open_breaker_goes_straight_to_vavoo() {
    let rig = Rig::new().await;
    let vavoo = rig.vavoo();
    let snap = rig.channel(&[("dlive://469/1", P1), (&vavoo, SourceOrigin::Vavoo)]);
    for _ in 0..4 {
        rig.dlive().note_outage();
    }
    assert!(rig.dlive().breaker_open());
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 1);
    assert_eq!(hits(&rig.fake.edge_hits), 0, "dlive not even asked");
    assert!(!cooling(&snap, 0), "skipped, not blamed");
}

#[tokio::test]
async fn an_open_breaker_still_plays_a_dlive_only_channel() {
    let rig = Rig::new().await;
    rig.channel(&[("dlive://469/1", P1)]);
    rig.dlive().note_ok();
    for _ in 0..4 {
        rig.dlive().note_outage();
    }
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 0);
    assert_eq!(hits(&rig.fake.edge_hits), 1);
}

#[tokio::test]
async fn unreachable_edges_open_the_breaker() {
    let rig = Rig::new().await;
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead = closed.local_addr().unwrap();
    drop(closed);
    rig.dlive().edge.write().unwrap().as_mut().unwrap().template =
        format!("http://{dead}/premium{{id}}/index.m3u8");
    // The fake embed would teach the live edge back.
    *rig.dlive().edge_attempt.lock().unwrap() = Some(Instant::now());
    let vavoo = rig.vavoo();
    let snap = rig.channel(&[("dlive://469/1", P1), (&vavoo, SourceOrigin::Vavoo)]);
    for _ in 0..4 {
        let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
        assert_eq!(mp.source_index, 1);
        snap.health[0][0].mark_success();
        snap.active_source[0].store(0, Ordering::Relaxed);
    }
    assert!(rig.dlive().breaker_open(), "four outages in a row");
}

#[tokio::test]
async fn a_403_on_a_signed_url_re_resolves_once() {
    let rig = Rig::new().await;
    rig.fake.sig_mode.store(1, Ordering::SeqCst);
    rig.cache_embed(469, 2);
    let vavoo = rig.vavoo();
    rig.channel(&[("dlive://469/2", P2), (&vavoo, SourceOrigin::Vavoo)]);
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 0, "the fresh signature plays");
    assert_eq!(hits(&rig.fake.embed_pages), 2);
    assert_eq!(hits(&rig.fake.site_pages), 0, "no dlive.sx load");
    let (ua, referer, _) = rig.dlive().cached_headers(469, 2);
    assert_eq!(ua.as_deref(), Some(UA));
    assert_eq!(referer, Some(format!("http://{}/", rig.addr)));
}

#[tokio::test]
async fn a_403_twice_hands_over_to_the_next_source() {
    let rig = Rig::new().await;
    rig.fake.sig_mode.store(2, Ordering::SeqCst);
    rig.cache_embed(469, 2);
    let vavoo = rig.vavoo();
    let snap = rig.channel(&[("dlive://469/2", P2), (&vavoo, SourceOrigin::Vavoo)]);
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 1);
    assert_eq!(hits(&rig.fake.embed_pages), 2, "re-resolved once only");
    assert!(cooling(&snap, 0));
}

#[tokio::test]
async fn an_unscraped_embed_is_skipped_then_fetched_in_the_background() {
    let rig = Rig::new().await;
    let vavoo = rig.vavoo();
    let snap = rig.channel(&[("dlive://469/2", P2), (&vavoo, SourceOrigin::Vavoo)]);
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 1);
    assert!(!cooling(&snap, 0), "not the source's fault");
    for _ in 0..200 {
        if rig.dlive().fresh_embed((469, 2)).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(matches!(
        rig.dlive().fresh_embed((469, 2)),
        Some(Embed::Found(_))
    ));
    assert_eq!(hits(&rig.fake.site_pages), 1, "one page load, once");
    let resolved = rig
        .dlive()
        .resolve(469, 2, ResolveMode::default())
        .await
        .unwrap();
    assert!(resolved.url.contains("/hls/x.m3u8?s=sig1"));
}

#[tokio::test]
async fn a_dlive_only_channel_scrapes_its_embed_inline() {
    let rig = Rig::new().await;
    rig.channel(&[("dlive://469/2", P2)]);
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 0);
    assert_eq!(hits(&rig.fake.site_pages), 1);
}

/// The first proxied URI of a rewritten playlist, as `(c, src, u, s)`.
fn first_proxied(body: &str) -> (String, Option<String>, String, String) {
    let line = body
        .lines()
        .find(|l| l.starts_with("/api/livetv/proxy?"))
        .unwrap();
    let url = Url::parse(&format!("http://iris{line}")).unwrap();
    let get = |k: &str| {
        url.query_pairs()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.into_owned())
    };
    (
        get("c").unwrap(),
        get("src"),
        get("u").unwrap(),
        get("s").unwrap(),
    )
}

async fn proxy_wrapped(
    rig: &Rig,
    body: &str,
) -> Result<(Vec<u8>, String), crate::live_tv::LiveTvError> {
    let (c, src, u, s) = first_proxied(body);
    let ProxiedResponse {
        resp,
        from_dlive,
        source,
        ..
    } = rig
        .svc
        .proxy_fetch(&c, src.as_deref(), &u, &s)
        .await
        .unwrap();
    assert!(from_dlive);
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    assert_eq!(ct.as_deref(), Some("image/png"));
    rig.svc
        .wrapped_segment(&c, source, resp, ct, from_dlive)
        .await
}

#[tokio::test]
async fn wrapped_segments_unwrap_and_bad_ones_rotate_to_vavoo() {
    let rig = Rig::new().await;
    let vavoo = rig.vavoo();
    let snap = rig.channel(&[("dlive://469/1", P1), (&vavoo, SourceOrigin::Vavoo)]);
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 0);

    let (ts, ct) = proxy_wrapped(&rig, &mp.body).await.unwrap();
    assert_eq!(ct, "video/mp2t");
    assert_eq!(ts, unwrap::tests::ts());

    rig.fake.segment_mode.store(1, Ordering::SeqCst);
    for _ in 0..3 {
        assert!(proxy_wrapped(&rig, &mp.body).await.is_err());
    }
    assert!(cooling(&snap, 0), "demoted after three bad segments");
    assert_eq!(snap.active_source[0].load(Ordering::Relaxed), 1);
    let next = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(next.source_index, 1, "the next reload plays Vavoo");
}

#[tokio::test]
async fn a_player_6_token_is_renewed_and_swapped_into_held_uris() {
    let rig = Rig::new().await;
    let stale = token_for("France2", epoch_s() - 10);
    rig.dlive().store(
        (469, 6),
        &Resolved {
            url: format!("http://{}/wide/France2/index.m3u8?token={stale}", rig.addr),
            user_agent: Some(UA.into()),
            referrer: None,
            origin: None,
            kind: Kind::Token {
                slug: "France2".into(),
                token: stale.clone(),
                refresh_url: format!("http://{}/api/refresh_token.php", rig.addr),
            },
            valid_until: epoch_s() - 70,
        },
    );
    let held = Url::parse(&format!(
        "http://{}/wide/France2/tracks-v1a1/2026/10/06-05120.ts?token={stale}",
        rig.addr
    ))
    .unwrap();
    let current = rig.dlive().refresh_upstream(469, 6, &held).await.unwrap();
    let token = parse::query_param(current.as_str(), "token")
        .unwrap()
        .to_string();
    assert_ne!(token, stale);
    assert!(parse::wide_token_expiry(&token).unwrap() > epoch_s());
    assert!(current.path().ends_with("/06-05120.ts"));
    assert!(rig.dlive().refresh_upstream(469, 6, &held).await.is_some());
    assert_eq!(hits(&rig.fake.refreshes), 1, "renewed once, then cached");
}

#[tokio::test]
async fn dlive_page_loads_stop_at_the_budget() {
    let rig = Rig::with(|cfg| cfg.dlive.page_budget = 2).await;
    for id in [469, 470] {
        assert!(rig.dlive().fetch_embed((id, 2)).await.is_ok());
    }
    assert!(matches!(
        rig.dlive().fetch_embed((950, 2)).await,
        Err(ResolveError::Skip(_))
    ));
    assert_eq!(hits(&rig.fake.site_pages), 2);
    rig.dlive().prefetch([(951, 2), (952, 6)]);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(hits(&rig.fake.site_pages), 2, "background loads too");
}

#[tokio::test]
async fn an_unreachable_dlive_sx_opens_the_breaker() {
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead = closed.local_addr().unwrap();
    drop(closed);
    let dlive = Arc::new(Dlive::new(
        config(format!("http://{dead}")).dlive,
        reqwest::Client::new(),
    ));
    assert!(matches!(
        dlive.fetch_embed((469, 2)).await,
        Err(ResolveError::Outage(_))
    ));
    assert!(dlive.breaker_open());
    assert!(matches!(
        dlive.resolve(469, 1, ResolveMode::default()).await,
        Err(ResolveError::Skip(_))
    ));
    assert!(dlive.index_within(Duration::from_secs(2)).await.is_none());
}

#[tokio::test]
async fn transcode_never_reads_a_dlive_source() {
    let rig = Rig::new().await;
    let vavoo = rig.vavoo();
    let snap = rig.channel(&[("dlive://469/1", P1), (&vavoo, SourceOrigin::Vavoo)]);
    let ch = &snap.channels[0];
    let now = crate::live_tv::epoch_ms();
    assert_eq!(
        crate::live_tv::transcode_source(ch, &snap.health[0], 0, now),
        Some(1)
    );
    let only = rig.channel(&[("dlive://469/1", P1)]);
    assert_eq!(
        crate::live_tv::transcode_source(&only.channels[0], &only.health[0], 0, now),
        None
    );
}

/// Live end-to-end resolution against dlive.sx (network, ~6 dlive.sx page
/// loads) — run explicitly with
/// `cargo test -p iris-api dlive_live -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "hits the live dlive.sx"]
async fn dlive_live_resolve_and_segment() {
    let mut cfg = iris_config::LiveTvConfig::default().dlive;
    cfg.enabled = true;
    cfg.countries = HashMap::from([("fr".to_string(), vec![950])]);
    let http = reqwest::Client::builder()
        .dns_resolver(Arc::new(crate::live_tv::dns::DohResolver::new().unwrap()))
        .build()
        .unwrap();
    let dlive = Arc::new(Dlive::new(cfg, http.clone()));
    let entries = dlive.entries("fr").await;
    eprintln!("dlive fr entries: {entries:?}");
    assert!(
        !entries.is_empty() && !entries[0].1.is_empty(),
        "list + id 950"
    );
    let mode = ResolveMode {
        bypass_breaker: false,
        inline_embed: true,
    };
    for player in [1u8, 2, 6] {
        let resolved = dlive.resolve(950, player, mode).await;
        eprintln!("player {player}: {resolved:?}");
        let resolved = resolved.expect("resolves");
        let mut req = http.get(&resolved.url);
        if let Some(ua) = &resolved.user_agent {
            req = req.header(reqwest::header::USER_AGENT, ua);
        }
        if let Some(origin) = &resolved.origin {
            req = req.header(reqwest::header::ORIGIN, origin);
        }
        let playlist = req.send().await.unwrap().text().await.unwrap();
        assert!(
            playlist.starts_with("#EXTM3U"),
            "player {player}: {playlist}"
        );
        if player == 1 {
            let segment = playlist
                .lines()
                .rfind(|l| l.starts_with("http"))
                .expect("a segment");
            let bytes = http
                .get(segment)
                .send()
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap();
            let ts = unwrap::unwrap_segment(&bytes).expect("TS inside");
            assert_eq!(ts[0], 0x47);
        }
    }
}

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("iris-dlive-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[tokio::test]
async fn scraped_embeds_and_the_edge_survive_a_restart() {
    let dir = scratch_dir("persist");
    let cfg = config("http://127.0.0.1:9".into()).dlive;
    let first = Dlive::new(cfg.clone(), reqwest::Client::new());
    first.persist_at(&dir);
    first.embeds.write().unwrap().insert(
        (469, 2),
        (
            Embed::Found("https://embed.example/e/abc".into()),
            epoch_s(),
        ),
    );
    first
        .embeds
        .write()
        .unwrap()
        .insert((469, 6), (Embed::Absent, epoch_s()));
    *first.edge.write().unwrap() = Some(Edge {
        template: "https://edge.example/premium{id}/index.m3u8".into(),
        embed_url: "https://embed.example/daddy.php?id=51".into(),
        token: None,
        origin: None,
    });
    first.save();
    let file = dir.join("dlive.json");
    for _ in 0..200 {
        if file.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let second = Dlive::new(cfg, reqwest::Client::new());
    second.persist_at(&dir);
    assert!(
        matches!(second.fresh_embed((469, 2)), Some(Embed::Found(url)) if url == "https://embed.example/e/abc")
    );
    assert!(matches!(second.fresh_embed((469, 6)), Some(Embed::Absent)));
    assert_eq!(
        second
            .edge
            .read()
            .unwrap()
            .as_ref()
            .map(|e| e.template.as_str()),
        Some("https://edge.example/premium{id}/index.m3u8"),
        "Player 1 plays at once after a restart"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_saves_keep_the_newest_state_whole() {
    let dir = scratch_dir("saves");
    let file = dir.join("dlive.json");
    let state = |n: u32| Persisted {
        edge: None,
        embeds: (0..n)
            .map(|id| PersistedEmbed {
                id,
                player: 2,
                url: Some(format!("https://embed.example/e/{id}")),
                at: 0,
            })
            .collect(),
    };
    let saves = Arc::new(Saves::default());
    let threads: Vec<_> = (1..=16u64)
        .map(|seq| {
            let (saves, file) = (saves.clone(), file.clone());
            std::thread::spawn(move || {
                saves
                    .write(&file, &state(u32::try_from(seq).unwrap() * 50), seq)
                    .unwrap()
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let read: Persisted = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        read.embeds.len(),
        16 * 50,
        "the newest save is the one kept"
    );
    assert!(
        !saves.write(&file, &state(1), 3).unwrap(),
        "an older save never overwrites a newer one"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_stale_embed_still_serves_and_asks_for_a_refresh() {
    let dlive = Dlive::new(
        config("http://127.0.0.1:9".into()).dlive,
        reqwest::Client::new(),
    );
    let old = epoch_s() - i64::try_from(dlive.cfg.embed_cache_hours * 3600 + 60).unwrap();
    dlive.embeds.write().unwrap().insert(
        (469, 2),
        (Embed::Found("https://embed.example/e/abc".into()), old),
    );
    assert!(
        dlive.fresh_embed((469, 2)).is_none(),
        "past its age: the warm-up refreshes it"
    );
    assert!(
        matches!(dlive.embed((469, 2)), Some((Embed::Found(_), true))),
        "yet it still serves"
    );
}

#[test]
fn a_huge_embed_cache_setting_means_forever() {
    let mut cfg = config("http://127.0.0.1:9".into()).dlive;
    cfg.embed_cache_hours = u64::MAX;
    let dlive = Dlive::new(cfg, reqwest::Client::new());
    dlive
        .embeds
        .write()
        .unwrap()
        .insert((469, 2), (Embed::Absent, 0));
    assert!(matches!(
        dlive.embed((469, 2)),
        Some((Embed::Absent, false))
    ));
}

#[test]
fn an_unreadable_state_file_starts_empty() {
    let dir = scratch_dir("bad");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("dlive.json"), b"{not json").unwrap();
    let dlive = Dlive::new(
        config("http://127.0.0.1:9".into()).dlive,
        reqwest::Client::new(),
    );
    dlive.persist_at(&dir);
    assert!(dlive.embed((469, 2)).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

fn listed(tvg: &str, name: &str, url: &str) -> M3uEntry {
    M3uEntry {
        name: name.to_string(),
        attrs: HashMap::from([("tvg-id".to_string(), tvg.to_string())]),
        vlc_opts: HashMap::new(),
        kodi_props: HashMap::new(),
        url: url.to_string(),
    }
}

fn merge_index() -> Index {
    Index {
        names: HashMap::from([
            (469, "TF1 France".to_string()),
            (51, "ABC USA".to_string()),
            (900, "Eurosport 1".to_string()),
            (901, "Hot 18+".to_string()),
            (902, "Nowhere TV".to_string()),
            (116, "beIN SPORTS 1 France".to_string()),
        ]),
    }
}

/// `country`'s list after the merge: its own entries, the allow-list, then
/// the rest of the dlive list by name.
fn merged_list(country: &str, own: Vec<M3uEntry>, allowed: &[u32]) -> Vec<Channel> {
    let index = merge_index();
    let mut lists = vec![(SourceOrigin::IptvOrg, own)];
    lists.extend(entries_for(&index, allowed, &[1], country));
    lists.extend(merged_entries_for(&index, allowed, &[1], country));
    crate::live_tv::channels::build_channels(&lists, None)
}

fn dlive_urls(ch: &Channel) -> Vec<&str> {
    ch.sources
        .iter()
        .filter(|s| s.url.starts_with("dlive://"))
        .map(|s| s.url.as_str())
        .collect()
}

#[test]
fn a_country_word_restricts_the_merge_to_that_country() {
    let fr = merged_list(
        "fr",
        vec![listed("TF1.fr@SD", "TF1 (720p)", "http://x/tf1")],
        &[],
    );
    assert_eq!(dlive_urls(&fr[0]), vec!["dlive://469/1"]);
    let us = merged_list(
        "us",
        vec![
            listed("", "TF1", "http://us/tf1"),
            listed("ABC.us", "ABC", "http://us/abc"),
        ],
        &[],
    );
    let tf1 = us.iter().find(|c| c.id == "tf1").unwrap();
    assert!(dlive_urls(tf1).is_empty(), "TF1 France stays out of us");
    let abc = us.iter().find(|c| c.id == "abc").unwrap();
    assert_eq!(dlive_urls(abc), vec!["dlive://51/1"]);
}

#[test]
fn an_unsuffixed_name_merges_wherever_it_matches_exactly() {
    for country in ["fr", "gb"] {
        let list = merged_list(
            country,
            vec![listed("Eurosport1.fr", "Eurosport 1", "http://x/es1")],
            &[],
        );
        assert_eq!(list.len(), 1, "{country}: no dlive-only channel");
        assert_eq!(dlive_urls(&list[0]), vec!["dlive://900/1"], "{country}");
        assert_eq!(
            list[0].sources.last().unwrap().url,
            "dlive://900/1",
            "a merged source is the last fallback"
        );
    }
}

#[test]
fn an_unmatched_or_adult_dlive_channel_is_not_used() {
    let list = merged_list(
        "fr",
        vec![
            listed("", "Hot 18+", "http://x/hot"),
            listed("TF1.fr", "TF1", "http://x/tf1"),
        ],
        &[],
    );
    assert_eq!(list.len(), 2, "Nowhere TV created nowhere");
    let hot = list.iter().find(|c| c.name == "Hot 18+").unwrap();
    assert!(dlive_urls(hot).is_empty(), "18+ excluded");
    assert!(
        !list
            .iter()
            .flat_map(|c| &c.sources)
            .any(|s| s.url == "dlive://902/1")
    );
}

#[test]
fn the_allow_list_still_creates_dlive_only_channels() {
    let list = merged_list("fr", vec![listed("TF1.fr", "TF1", "http://x/tf1")], &[116]);
    let bein = list.iter().find(|c| c.name == "beIN SPORTS 1").unwrap();
    assert_eq!(dlive_urls(bein), vec!["dlive://116/1"]);
    assert_eq!(
        bein.sources[0].origin,
        SourceOrigin::Dlive { rank: 0 },
        "listed, not merged: it keeps the allow-list rank"
    );
    let merged = merged_entries_for(&merge_index(), &[116], &[1], "fr");
    assert!(
        merged[0].1.iter().all(|e| e.url != "dlive://116/1"),
        "an allow-listed id is not offered twice"
    );
}

#[test]
fn the_warm_up_scrapes_the_allow_list_then_recent_zaps_only() {
    let mut cfg = config("http://127.0.0.1:9".into()).dlive;
    cfg.players = vec![1, 2];
    cfg.countries = HashMap::from([("fr".to_string(), vec![469])]);
    let dlive = Arc::new(Dlive::new(cfg, reqwest::Client::new()));
    let index = merge_index();
    assert_eq!(dlive.warm_next(&index), Some((469, 2)));
    dlive
        .embeds
        .write()
        .unwrap()
        .insert((469, 2), (Embed::Absent, epoch_s()));
    assert_eq!(dlive.warm_next(&index), None, "never the merged set");
    dlive.opened.lock().unwrap().insert(900, Instant::now());
    assert_eq!(
        dlive.warm_next(&index),
        Some((900, 2)),
        "a channel zapped to"
    );
}

/// Player 1's edge serves nothing without a token; the token comes from its
/// API, asked with the embed page's `Origin`, and is reused while fresh.
#[tokio::test]
async fn player_1_plays_with_a_token_asked_with_the_embed_origin() {
    let rig = Rig::new().await;
    rig.tokened();
    let dlive = rig.dlive();
    let bare = format!("http://{}/edge/premium469/index.m3u8", rig.addr);
    let resp = reqwest::get(&bare).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "no token, no playlist"
    );

    let p1 = dlive.resolve(469, 1, ResolveMode::default()).await.unwrap();
    assert_eq!(p1.url, format!("{bare}?t=tok1"));
    assert!(p1.is_signed(), "a 403 re-resolves it");
    let origin = format!("http://{}", rig.addr);
    assert_eq!(p1.origin.as_deref(), Some(origin.as_str()));
    assert_eq!(
        dlive.cached_headers(469, 1).2.as_deref(),
        Some(origin.as_str()),
        "for proxied refreshes"
    );
    let now = epoch_s();
    assert!(
        (now + 58..=now + 60).contains(&p1.valid_until),
        "renewed at half its 120 s"
    );
    assert_eq!(
        dlive
            .resolve(469, 1, ResolveMode::default())
            .await
            .unwrap()
            .url,
        p1.url,
        "reused"
    );
    assert_eq!(hits(&rig.fake.edge_tokens), 1);
    let vavoo = rig.vavoo();
    rig.channel(&[("dlive://469/1", P1), (&vavoo, SourceOrigin::Vavoo)]);
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 0, "the election sends the Origin too");
}

/// A proxied playlist refresh carries the embed's `Origin` and the live
/// token, as the edge demands both.
#[tokio::test]
async fn a_proxied_player_1_playlist_carries_the_origin_and_token() {
    let rig = Rig::new().await;
    rig.tokened();
    rig.channel(&[("dlive://469/1", P1)]);
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 0);
    let p1 = rig
        .dlive()
        .resolve(469, 1, ResolveMode::default())
        .await
        .unwrap();
    let key = "fr:tf1";
    let tag = crate::live_tv::proxy::SourceTag::of(0, "dlive://469/1").to_string();
    let upstream = Url::parse(&p1.url).unwrap();
    let proxied = crate::live_tv::proxy::proxied_url(
        key,
        crate::live_tv::proxy::SourceTag::parse(&tag),
        &upstream,
        &rig.svc.inner.signer,
    );
    let (c, src, u, s) = first_proxied(&format!("#EXTM3U\n{proxied}\n"));
    let ProxiedResponse { resp, .. } = rig
        .svc
        .proxy_fetch(&c, src.as_deref(), &u, &s)
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());
}

/// Asked from anywhere but the embed's origin, the token endpoint says no:
/// an outage, and the edge is re-read from the embed, which fixes it.
#[tokio::test]
async fn a_token_refused_for_its_origin_relearns_the_edge() {
    let rig = Rig::new().await;
    rig.tokened();
    let dlive = rig.dlive();
    dlive.edge.write().unwrap().as_mut().unwrap().origin = Some("https://elsewhere.example".into());
    assert!(matches!(
        dlive.resolve(469, 1, ResolveMode::default()).await,
        Err(ResolveError::Outage(_))
    ));
    assert_eq!(hits(&rig.fake.edge_tokens), 0);
    rig.edge_learned().await;
    assert_eq!(
        dlive.edge_generation(),
        1,
        "the embed's origin learned back"
    );
    assert!(dlive.resolve(469, 1, ResolveMode::default()).await.is_ok());
}

/// Past its half-life the token is asked again, and the playlist URL a
/// viewer holds is swapped to it; segments elsewhere are left alone.
#[tokio::test]
async fn a_stale_edge_token_is_renewed_into_held_playlists() {
    let rig = Rig::new().await;
    rig.tokened();
    let dlive = rig.dlive();
    let p1 = dlive.resolve(469, 1, ResolveMode::default()).await.unwrap();
    let held = Url::parse(&p1.url).unwrap();
    assert_eq!(
        dlive.refresh_upstream(469, 1, &held).await,
        None,
        "still the live token"
    );
    dlive.store(
        (469, 1),
        &Resolved {
            valid_until: epoch_s(),
            ..p1
        },
    );
    let current = dlive.refresh_upstream(469, 1, &held).await.unwrap();
    assert_eq!(current.as_str(), held.as_str().replace("t=tok1", "t=tok2"));
    assert_eq!(hits(&rig.fake.edge_tokens), 2);
    let tiktok = Url::parse("https://p16.example/x.image?dr=1&t=4d5b0474").unwrap();
    assert_eq!(dlive.refresh_upstream(469, 1, &tiktok).await, None);
}

/// A token the edge revoked early: one fresh token, then the next source.
#[tokio::test]
async fn a_403_on_player_1_asks_one_fresh_token() {
    let rig = Rig::new().await;
    rig.tokened();
    let vavoo = rig.vavoo();
    let snap = rig.channel(&[("dlive://469/1", P1), (&vavoo, SourceOrigin::Vavoo)]);
    rig.dlive()
        .resolve(469, 1, ResolveMode::default())
        .await
        .unwrap();
    rig.fake.edge_revoked.store(1, Ordering::SeqCst);
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 0, "tok1 refused, tok2 plays");
    assert_eq!(hits(&rig.fake.edge_tokens), 2);

    rig.fake.edge_revoked.store(usize::MAX, Ordering::SeqCst);
    rig.dlive().invalidate(469, 1);
    snap.active_source[0].store(0, Ordering::Relaxed);
    snap.health[0][0].mark_success();
    let mp = rig.svc.master_playlist("fr", "tf1").await.unwrap();
    assert_eq!(mp.source_index, 1);
    assert_eq!(hits(&rig.fake.edge_tokens), 4, "re-resolved once only");
    assert!(cooling(&snap, 0));
}

/// An embed that asks no token (or a state file from before tokens) plays
/// the bare playlist, and is re-read until it does ask one.
#[tokio::test]
async fn a_tokenless_edge_plays_bare_and_picks_up_a_token() {
    let rig = Rig::new().await;
    let dlive = rig.dlive();
    *dlive.edge_attempt.lock().unwrap() = Some(Instant::now());
    let p1 = dlive.resolve(469, 1, ResolveMode::default()).await.unwrap();
    assert_eq!(
        (p1.url.as_str(), &p1.kind),
        (
            format!("http://{}/edge/premium469/index.m3u8", rig.addr).as_str(),
            &Kind::Plain
        )
    );
    assert_eq!(
        hits(&rig.fake.dembed_pages),
        0,
        "re-read at most every EDGE_RELEARN_EVERY"
    );

    rig.fake.edge_wants_token.store(true, Ordering::SeqCst);
    *dlive.edge_attempt.lock().unwrap() = None;
    assert_eq!(
        dlive
            .resolve(469, 1, ResolveMode::default())
            .await
            .unwrap()
            .kind,
        Kind::Plain
    );
    rig.edge_learned().await;
    assert_eq!(hits(&rig.fake.dembed_pages), 1);
    let p1 = dlive.resolve(469, 1, ResolveMode::default()).await.unwrap();
    assert!(p1.url.ends_with("?t=tok1"), "{}", p1.url);
}

/// A state file written before tokens still loads (the edge lacks them).
#[test]
fn an_edge_saved_before_tokens_still_loads() {
    let old = r#"{"edge":{"template":"https://e.example/premium{id}/index.m3u8","embed_url":"https://d.example/x"},"embeds":[]}"#;
    let p: Persisted = serde_json::from_str(old).unwrap();
    let edge = p.edge.unwrap();
    assert_eq!((edge.token, edge.origin), (None, None));
}
