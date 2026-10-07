//! `tmdb-trust` dry run / apply: re-evaluate every collection's TMDB match
//! through the trust gate ([`crate::tmdb_trust`]) and report, per
//! collection, whether its cover stays, changes or goes. Writes only with
//! `apply`; never runs on its own.

use std::fmt::Write as _;

use iris_db::SqlitePool;
use iris_db::collections::CollectionRow;

use crate::tmdb::{TmdbClient, TmdbKind};
use crate::tmdb_trust::{self, Reason, TmdbSource, Trust};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Trusted, same id as today.
    Keep,
    /// Trusted, another id than today's (or none today).
    Change,
    /// No trusted signal: today's cover goes.
    Lose,
    /// No cover today, none trusted either.
    None,
    /// Not evaluated: admin-set, or TMDB unreachable (re-run later).
    Skipped,
}

impl Verdict {
    const fn label(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Change => "change",
            Self::Lose => "lose",
            Self::None => "none",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuditRow {
    pub collection_id: uuid::Uuid,
    pub display_title: String,
    pub kind: String,
    pub verdict: Verdict,
    pub old_id: Option<i64>,
    pub old_title: Option<String>,
    pub new_id: Option<i64>,
    pub new_title: Option<String>,
    pub trust: Option<Trust>,
    pub reason: String,
}

pub struct Report {
    pub rows: Vec<AuditRow>,
    pub applied: bool,
    /// Resolve-cache rows flushed by `apply`.
    pub flushed: u64,
}

/// Evaluate every collection; with `apply`, write the outcome and flush the
/// resolve cache's negative and fuzzy entries.
pub async fn run(pool: &SqlitePool, tmdb: &TmdbClient, apply: bool) -> anyhow::Result<Report> {
    run_with(pool, tmdb, apply).await
}

pub(crate) async fn run_with<S: TmdbSource>(
    pool: &SqlitePool,
    tmdb: &S,
    apply: bool,
) -> anyhow::Result<Report> {
    let mut rows = Vec::new();
    for c in iris_db::collections::list_all(pool).await? {
        rows.push(audit_one(pool, tmdb, &c).await?);
    }
    let mut flushed = 0;
    if apply {
        for r in &rows {
            write_row(pool, r).await?;
        }
        flushed =
            iris_db::tmdb_cache::flush_untrusted(pool, tmdb_trust::STRICT_CACHE_PREFIX).await?;
    }
    Ok(Report {
        rows,
        applied: apply,
        flushed,
    })
}

async fn audit_one<S: TmdbSource>(
    pool: &SqlitePool,
    tmdb: &S,
    c: &CollectionRow,
) -> anyhow::Result<AuditRow> {
    let kind = TmdbKind::from_wire(&c.kind);
    let mut row = AuditRow {
        collection_id: c.id,
        display_title: c.display_title.clone(),
        kind: c.kind.clone(),
        verdict: Verdict::Skipped,
        old_id: c.tmdb_id,
        old_title: None,
        new_id: None,
        new_title: None,
        trust: None,
        reason: String::new(),
    };
    if let Some(old) = c.tmdb_id.and_then(|i| u64::try_from(i).ok()) {
        row.old_title = tmdb
            .lookup(old, kind.unwrap_or(TmdbKind::Movie))
            .await
            .flatten()
            .map(|m| m.title);
    }
    if c.tmdb_trust.as_deref().and_then(Trust::from_stored) == Some(Trust::Admin) {
        row.reason = "admin-set, never re-evaluated".into();
        row.new_id = c.tmdb_id;
        row.trust = Some(Trust::Admin);
        return Ok(row);
    }
    let trackers: Vec<u64> = iris_db::collections::tracker_tmdb_ids(pool, c.id)
        .await?
        .into_iter()
        .filter_map(|i| u64::try_from(i).ok())
        .collect();
    let unreachable = |mut row: AuditRow| {
        row.reason = "TMDB unreachable, re-run later".into();
        row
    };
    let Ok(eval) =
        tmdb_trust::evaluate(pool, tmdb, &c.display_title, kind, c.is_anime, &trackers).await
    else {
        return Ok(unreachable(row));
    };
    let mut decision = eval.decision;
    row.reason = eval.reason.to_string();
    row.new_title = eval.trusted_title();
    let undecided = matches!(
        eval.reason,
        Reason::NoSignal | Reason::NoTitle | Reason::TrackerImplausible(_)
    );
    if let (true, Some(old)) = (undecided, c.tmdb_id.and_then(|i| u64::try_from(i).ok())) {
        match tmdb_trust::verify_current(tmdb, &c.display_title, kind, c.is_anime, old).await {
            Ok(Some(title)) => {
                decision = Some((old, Trust::Scene));
                row.reason = Reason::CurrentVerified.to_string();
                row.new_title = Some(title);
            }
            Ok(None) => {}
            Err(_) => return Ok(unreachable(row)),
        }
    }
    if let Some((id, trust)) = decision {
        let id = i64::try_from(id)?;
        row.new_id = Some(id);
        row.trust = Some(trust);
        row.verdict = if c.tmdb_id == Some(id) {
            if row.old_title.is_none() {
                row.old_title.clone_from(&row.new_title);
            }
            Verdict::Keep
        } else {
            Verdict::Change
        };
    } else {
        row.verdict = if c.tmdb_id.is_some() {
            Verdict::Lose
        } else {
            Verdict::None
        };
        if let (Some(scene), Reason::TrackerDisagrees { .. }) = (&eval.scene, &eval.reason) {
            let _ = write!(row.reason, " (SCENE: \"{}\")", scene.title);
        }
    }
    Ok(row)
}

async fn write_row(pool: &SqlitePool, r: &AuditRow) -> Result<(), sqlx::Error> {
    match (r.verdict, r.new_id, r.trust) {
        (Verdict::Keep | Verdict::Change, Some(id), Some(trust)) => {
            iris_db::collections::set_trusted_tmdb(pool, r.collection_id, id, trust.as_str()).await
        }
        (Verdict::Lose, ..) => iris_db::collections::clear_tmdb_id(pool, r.collection_id).await,
        _ => Ok(()),
    }
}

fn id_title(id: Option<i64>, title: Option<&str>) -> String {
    match (id, title) {
        (Some(id), Some(t)) => format!("{id} \"{t}\""),
        (Some(id), None) => format!("{id} (title unknown)"),
        (None, _) => "(no cover)".into(),
    }
}

/// The human-readable report the bin prints.
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    let count = |v: Verdict| report.rows.iter().filter(|r| r.verdict == v).count();
    let mode = if report.applied {
        "APPLIED"
    } else {
        "dry run, nothing written (pass --apply to write)"
    };
    let _ = writeln!(
        out,
        "TMDB trust — {} collections — {mode}",
        report.rows.len()
    );
    let verdicts = [
        Verdict::Keep,
        Verdict::Change,
        Verdict::Lose,
        Verdict::None,
        Verdict::Skipped,
    ];
    let summary: Vec<String> = verdicts
        .iter()
        .map(|v| format!("{} {}", v.label(), count(*v)))
        .collect();
    let _ = writeln!(out, "  {}", summary.join(" · "));
    if report.applied {
        let _ = writeln!(out, "  resolve cache: {} entries flushed", report.flushed);
    }
    for v in verdicts {
        let rows: Vec<&AuditRow> = report.rows.iter().filter(|r| r.verdict == v).collect();
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n{}", v.label().to_uppercase());
        for r in rows {
            let old = id_title(r.old_id, r.old_title.as_deref());
            let ids = if matches!(v, Verdict::Change | Verdict::Lose) {
                format!("{old} → {}", id_title(r.new_id, r.new_title.as_deref()))
            } else {
                old
            };
            let trust = r
                .trust
                .map(|t| format!(" [{}]", t.as_str()))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "  {} ({}) — {ids} — {}{trust}",
                r.display_title, r.kind, r.reason
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmdb_trust::tests::{FakeTmdb, meta, suggestion};

    async fn collection(
        pool: &SqlitePool,
        title: &str,
        kind: &str,
        tmdb: Option<i64>,
    ) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO collections (id, tmdb_id, parsed_title_normalized, display_title, kind, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(id)
        .bind(tmdb)
        .bind(title.to_lowercase())
        .bind(title)
        .bind(kind)
        .bind(chrono::Utc::now())
        .execute(pool)
        .await
        .unwrap();
        id
    }

    async fn tracker_torrent(pool: &SqlitePool, collection: uuid::Uuid, tracker_id: i64) {
        let user = iris_db::test_support::make_user(pool).await;
        let t = iris_db::torrents::upsert(
            pool,
            iris_db::torrents::NewTorrent {
                infohash: format!("{collection}"),
                name: "x".into(),
                total_size_bytes: 1,
                source_provider: Some("p".into()),
                source_external_id: Some("e".into()),
                tracker_tmdb_id: Some(tracker_id),
                added_by: user,
            },
        )
        .await
        .unwrap();
        iris_db::torrents::set_collection(pool, &t.infohash, Some(collection))
            .await
            .unwrap();
    }

    fn verdict(r: &Report, title: &str) -> Verdict {
        r.rows
            .iter()
            .find(|r| r.display_title == title)
            .unwrap()
            .verdict
    }

    #[tokio::test]
    async fn dry_run_classifies_and_apply_writes() {
        let pool = iris_db::test_support::migrated_pool().await;
        let mut tmdb = FakeTmdb::default();
        tmdb.searches.insert(
            "dune".into(),
            vec![suggestion(438_631, TmdbKind::Movie, "Dune", 2021)],
        );
        tmdb.searches.insert(
            "midnight".into(),
            vec![suggestion(34, TmdbKind::Movie, "Midnight", 2021)],
        );
        tmdb.ids
            .insert(12, meta(12, TmdbKind::Movie, "Midnight Matinee", 1988));
        tmdb.ids
            .insert(77, meta(77, TmdbKind::Movie, "The Godfather", 1972));
        tmdb.ids
            .insert(2316, meta(2316, TmdbKind::Tv, "The Office", 2005));
        tmdb.titles.insert(
            2316,
            crate::tmdb::KnownTitles {
                titles: vec!["The Office".into(), "The Office (US)".into()],
                year: Some(2005),
                animation: false,
            },
        );
        tmdb.ids.insert(5, meta(5, TmdbKind::Tv, "Severance", 2022));
        tmdb.searches.insert(
            "severance".into(),
            vec![suggestion(95_396, TmdbKind::Tv, "Severance", 2022)],
        );

        let dune = collection(&pool, "Dune (2021)", "movie", Some(438_631)).await;
        let midnight = collection(&pool, "Midnight (2021)", "movie", Some(12)).await;
        let parrain = collection(&pool, "Le Parrain (1972)", "movie", Some(77)).await;
        collection(&pool, "Nothing Here (2020)", "movie", None).await;
        let sev = collection(&pool, "Severance", "tv", Some(95_396)).await;
        let office = collection(&pool, "The Office US", "tv", Some(2316)).await;
        tracker_torrent(&pool, sev, 5).await;

        let dry = run_with(&pool, &tmdb, false).await.unwrap();
        assert_eq!(verdict(&dry, "Dune (2021)"), Verdict::Keep);
        assert_eq!(verdict(&dry, "Midnight (2021)"), Verdict::Change);
        assert_eq!(verdict(&dry, "Le Parrain (1972)"), Verdict::Lose);
        assert_eq!(verdict(&dry, "Nothing Here (2020)"), Verdict::None);
        assert_eq!(
            verdict(&dry, "The Office US"),
            Verdict::Keep,
            "no search match, but one of its id's titles is the release's"
        );
        assert_eq!(
            verdict(&dry, "Severance"),
            Verdict::Lose,
            "tracker id 5 disagrees with the SCENE match"
        );
        let text = render(&dry);
        assert!(
            text.contains("12 \"Midnight Matinee\" → 34 \"Midnight\""),
            "{text}"
        );
        assert!(
            text.contains("keep 2 · change 1 · lose 2 · none 1 · skipped 0"),
            "{text}"
        );
        let untouched = iris_db::collections::get(&pool, parrain)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(untouched.tmdb_id, Some(77), "dry run writes nothing");
        assert_eq!(untouched.tmdb_trust, None);

        let applied = run_with(&pool, &tmdb, true).await.unwrap();
        assert!(applied.applied);
        let get = |id| {
            let pool = pool.clone();
            async move { iris_db::collections::get(&pool, id).await.unwrap().unwrap() }
        };
        let d = get(dune).await;
        assert_eq!(
            (d.tmdb_id, d.tmdb_trust.as_deref()),
            (Some(438_631), Some("scene"))
        );
        assert_eq!(get(midnight).await.tmdb_id, Some(34));
        let p = get(parrain).await;
        assert_eq!((p.tmdb_id, p.tmdb_trust), (None, None));
        assert_eq!(get(sev).await.tmdb_id, None);
        let o = get(office).await;
        assert_eq!(
            (o.tmdb_id, o.tmdb_trust.as_deref()),
            (Some(2316), Some("scene"))
        );
    }

    #[tokio::test]
    async fn offline_skips_and_writes_nothing() {
        let pool = iris_db::test_support::migrated_pool().await;
        let tmdb = FakeTmdb {
            offline: true,
            ..FakeTmdb::default()
        };
        let id = collection(&pool, "Arrival (2016)", "movie", Some(329_865)).await;
        let r = run_with(&pool, &tmdb, true).await.unwrap();
        assert_eq!(r.rows[0].verdict, Verdict::Skipped);
        let c = iris_db::collections::get(&pool, id).await.unwrap().unwrap();
        assert_eq!(c.tmdb_id, Some(329_865));
    }
}
