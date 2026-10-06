//! `parse-dryrun`: what a filename-parser change does to the library's
//! collection identities, before it ships. For every collection, the identity
//! its live torrents' names derive under the shipped parser and under
//! [`filename::parse`] (key, display title, kind, season), every rename, and
//! the merges a rename would cause. Read only.
//!
//! The shipped parser cut the last dot segment of every name as an extension
//! (`Dr. Stone S03E01 …` → `Dr`, `Movie.2021` lost its year); it is kept here,
//! as [`legacy_parse`], only to compare against.

use std::collections::HashMap;
use std::fmt::Write as _;

use iris_db::SqlitePool;
use iris_media::filename::{self, Parsed};
use uuid::Uuid;

/// The parser before known-extension stripping.
pub fn legacy_parse(name: &str) -> Option<Parsed> {
    filename::parse_stem(name.rsplit_once('.').map_or(name, |(stem, _)| stem))
}

/// A collection identity as the assign / heal paths derive it from names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub key: String,
    pub display: String,
    pub is_tv: bool,
    pub season: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Rename {
    pub collection_id: Uuid,
    pub kind: String,
    pub current_key: String,
    pub display_title: String,
    pub old: Option<Identity>,
    pub new: Option<Identity>,
    /// Another collection that already holds (or would also take) the new
    /// key: the heals skip the rename, a new grab joins that one.
    pub collides_with: Vec<(Uuid, String)>,
}

#[derive(Debug, Clone)]
pub struct TorrentDelta {
    pub collection_id: Option<Uuid>,
    pub name: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub collections: usize,
    pub torrents: usize,
    /// Over the live releases: what the heals act on.
    pub renames: Vec<Rename>,
    pub torrent_deltas: Vec<TorrentDelta>,
    /// Removed releases included: what grabbing one of them again would key.
    pub removed_torrents: usize,
    pub renames_with_removed: Vec<Rename>,
    pub removed_deltas: Vec<TorrentDelta>,
}

/// Identity of a collection from its members' names, the way the assign
/// path picks it: TV by the heal's consensus (longest structural key), a
/// movie from the first name that parses to a title.
pub fn derive(
    names: &[&str],
    is_tv: bool,
    is_anime: bool,
    parse: fn(&str) -> Option<Parsed>,
) -> Option<Identity> {
    let parsed = if is_tv {
        crate::collection_assign::consensus_identity_by(names.iter().copied(), parse)?
    } else {
        names
            .iter()
            .filter_map(|n| parse(n))
            .find(|p| !p.title.is_empty())?
    };
    let key = parsed.collection_key_kind(is_tv, is_anime);
    (!key.is_empty()).then(|| Identity {
        key,
        display: parsed.display_with_year(is_tv),
        is_tv: parsed.is_tv(),
        season: parsed.season,
    })
}

fn summary(p: Option<&Parsed>) -> String {
    p.map_or_else(
        || "(unparsed)".to_owned(),
        |p| {
            format!(
                "title={:?} year={:?} S={:?} E={:?} abs={:?} quality={:?} group={:?}",
                p.title, p.year, p.season, p.episode, p.absolute_episode, p.quality, p.group
            )
        },
    )
}

/// # Errors
/// A failed DB read.
pub async fn run(pool: &SqlitePool) -> Result<Report, sqlx::Error> {
    let cols = iris_db::collections::list_all(pool).await?;
    let mut report = Report {
        collections: cols.len(),
        ..Report::default()
    };
    let mut owners: Owners = HashMap::new();
    for c in &cols {
        if let Some(k) = c.parsed_title_normalized.as_deref() {
            owners
                .entry((c.kind.clone(), k.to_owned()))
                .or_default()
                .push((c.id, c.display_title.clone()));
        }
    }
    let mut renames = Vec::new();
    let mut with_removed = Vec::new();
    for c in &cols {
        let all: Vec<(String, bool)> = sqlx::query_as(
            "SELECT name, deleted_at IS NULL FROM torrents WHERE collection_id = ?1 ORDER BY added_at",
        )
        .bind(c.id)
        .fetch_all(pool)
        .await?;
        for (name, live) in &all {
            if *live {
                report.torrents += 1;
            } else {
                report.removed_torrents += 1;
            }
            let (old, new) = (legacy_parse(name), filename::parse(name));
            let (old, new) = (summary(old.as_ref()), summary(new.as_ref()));
            if old != new {
                let delta = TorrentDelta {
                    collection_id: Some(c.id),
                    name: name.clone(),
                    old,
                    new,
                };
                if *live {
                    report.torrent_deltas.push(delta);
                } else {
                    report.removed_deltas.push(delta);
                }
            }
        }
        let live: Vec<&str> = all
            .iter()
            .filter(|(_, live)| *live)
            .map(|(n, _)| n.as_str())
            .collect();
        let every: Vec<&str> = all.iter().map(|(n, _)| n.as_str()).collect();
        if let Some(r) = rename(c, &live) {
            renames.push(r);
        } else if live.is_empty()
            && let Some(r) = rename(c, &every)
        {
            with_removed.push(r);
        }
    }
    let renames = with_collisions(&owners, renames);
    report.renames_with_removed = with_collisions(&owners, with_removed);
    report.renames = renames;
    Ok(report)
}

fn rename(c: &iris_db::collections::CollectionRow, names: &[&str]) -> Option<Rename> {
    let is_tv = c.is_tv();
    let old = derive(names, is_tv, c.is_anime, legacy_parse);
    let new = derive(names, is_tv, c.is_anime, filename::parse);
    (old != new).then(|| Rename {
        collection_id: c.id,
        kind: c.kind.clone(),
        current_key: c.parsed_title_normalized.clone().unwrap_or_default(),
        display_title: c.display_title.clone(),
        old,
        new,
        collides_with: Vec::new(),
    })
}

type Owners = HashMap<(String, String), Vec<(Uuid, String)>>;

fn with_collisions(owners: &Owners, mut renames: Vec<Rename>) -> Vec<Rename> {
    let mut targets: Owners = HashMap::new();
    for r in &renames {
        if let Some(n) = &r.new {
            targets
                .entry((r.kind.clone(), n.key.clone()))
                .or_default()
                .push((r.collection_id, r.display_title.clone()));
        }
    }
    for r in &mut renames {
        let Some(n) = &r.new else { continue };
        let slot = (r.kind.clone(), n.key.clone());
        r.collides_with = owners
            .get(&slot)
            .into_iter()
            .chain(targets.get(&slot))
            .flatten()
            .filter(|(id, _)| *id != r.collection_id)
            .cloned()
            .collect();
        r.collides_with.sort();
        r.collides_with.dedup();
    }
    renames
}

fn identity(i: Option<&Identity>) -> String {
    i.map_or_else(
        || "(none)".to_owned(),
        |i| {
            format!(
                "key={:?} display={:?} kind={} season={:?}",
                i.key,
                i.display,
                if i.is_tv { "tv" } else { "movie" },
                i.season
            )
        },
    )
}

#[must_use]
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    let colliding = report
        .renames
        .iter()
        .filter(|r| !r.collides_with.is_empty())
        .count();
    let _ = writeln!(
        out,
        "{} collections, {} live torrents: {} collection identities change ({} collide with another collection), {} torrent parses change",
        report.collections,
        report.torrents,
        report.renames.len(),
        colliding,
        report.torrent_deltas.len()
    );
    let _ = writeln!(
        out,
        "removed releases: {} more torrents; {} collections with no live release would key differently on a re-grab ({} collide), {} removed-torrent parses change",
        report.removed_torrents,
        report.renames_with_removed.len(),
        report
            .renames_with_removed
            .iter()
            .filter(|r| !r.collides_with.is_empty())
            .count(),
        report.removed_deltas.len()
    );
    section(
        &mut out,
        "Collection identity changes, live releases (old parser -> new parser)",
        &report.renames,
    );
    section(
        &mut out,
        "Collections with no live release: identity from the removed ones",
        &report.renames_with_removed,
    );
    deltas(
        &mut out,
        "Torrent parse changes, live",
        &report.torrent_deltas,
    );
    deltas(
        &mut out,
        "Torrent parse changes, removed",
        &report.removed_deltas,
    );
    out
}

fn section(out: &mut String, title: &str, renames: &[Rename]) {
    let _ = writeln!(out, "\n# {title}");
    for r in renames {
        let _ = writeln!(
            out,
            "\n[{}] {} {:?} (stored key {:?})\n  old: {}\n  new: {}",
            r.kind,
            r.collection_id,
            r.display_title,
            r.current_key,
            identity(r.old.as_ref()),
            identity(r.new.as_ref())
        );
        for (id, title) in &r.collides_with {
            let _ = writeln!(out, "  MERGE/COLLIDE with {id} {title:?}");
        }
    }
}

fn deltas(out: &mut String, title: &str, deltas: &[TorrentDelta]) {
    let _ = writeln!(out, "\n# {title}");
    for d in deltas {
        let _ = writeln!(
            out,
            "\n{:?} (collection {})\n  old: {}\n  new: {}",
            d.name,
            d.collection_id
                .map_or_else(|| "-".to_owned(), |c| c.to_string()),
            d.old,
            d.new
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_legacy_parser_cut_the_last_dot_segment() {
        assert_eq!(legacy_parse("Dr. Stone S03E01 1080p").unwrap().title, "Dr");
        let names = ["Dr. Stone S03E01 1080p WEB"];
        assert!(
            derive(&names, true, false, legacy_parse).is_none(),
            "the season marker went with the cut"
        );
        let new = derive(&names, true, false, filename::parse).unwrap();
        assert_eq!((new.key.as_str(), new.season), ("dr stone", Some(3)));
    }

    #[tokio::test]
    async fn a_run_reports_each_identity_change_and_its_collisions() {
        let pool = iris_db::test_support::migrated_pool().await;
        let user = iris_db::test_support::make_user(&pool).await;
        let add = |name: &'static str, key: &'static str, display: &'static str| {
            let pool = pool.clone();
            async move {
                let c = iris_db::collections::find_or_create(
                    &pool,
                    key,
                    display,
                    iris_db::collections::Kind::Tv,
                    false,
                )
                .await
                .unwrap();
                let t = iris_db::torrents::upsert(
                    &pool,
                    iris_db::torrents::NewTorrent {
                        infohash: Uuid::new_v4().simple().to_string(),
                        name: name.to_owned(),
                        total_size_bytes: 1,
                        source_provider: None,
                        source_external_id: None,
                        tracker_tmdb_id: None,
                        added_by: user,
                    },
                )
                .await
                .unwrap();
                iris_db::torrents::set_collection(&pool, &t.infohash, Some(c.id))
                    .await
                    .unwrap();
                c
            }
        };
        let dr = add("Dr. Stone S03E01 1080p WEB", "dr", "Dr").await;
        let other = add("Dr.Stone.S02E01.1080p.WEB.x264-GRP", "dr stone", "Dr Stone").await;
        add("Show.S01E01.1080p.WEB.x264-GRP", "show", "Show").await;
        let report = run(&pool).await.unwrap();
        assert_eq!(report.collections, 3);
        assert_eq!(report.renames.len(), 1, "{:?}", report.renames);
        let r = &report.renames[0];
        assert_eq!(r.collection_id, dr.id);
        assert_eq!(r.new.as_ref().unwrap().key, "dr stone");
        assert_eq!(r.collides_with, [(other.id, "Dr Stone".to_owned())]);
        assert!(render(&report).contains("MERGE/COLLIDE"));
    }
}
