//! Re-evaluate every collection's TMDB match through the trust gate.
//!
//!   - `cargo run -p iris-api --bin tmdb-trust`                 → dry run (default)
//!   - `cargo run -p iris-api --bin tmdb-trust -- --apply`      → write it
//!   - `--db <path>` picks the database (default: `<data_dir>/iris.db` from
//!     the config); `--config` / `IRIS_CONFIG` the config file that supplies
//!     the TMDB key (`IRIS_TMDB__API_KEY` overrides it).
//!
//! Never runs on its own. See `docs/DEPLOYMENT.md`.

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "tmdb-trust",
    about = "TMDB trust gate: dry run (default) or --apply"
)]
struct Cli {
    /// Write the result: trusted ids + `tmdb_trust`, untrusted ids cleared,
    /// negative / fuzzy resolve-cache entries flushed.
    #[arg(long)]
    apply: bool,
    /// Explicit no-op, the default: report only.
    #[arg(long, conflicts_with = "apply")]
    dry_run: bool,
    /// `SQLite` database to evaluate.
    #[arg(long)]
    db: Option<PathBuf>,
    #[arg(short, long, env = "IRIS_CONFIG", default_value = "config/config.toml")]
    config: PathBuf,
    /// TMDB API key; else the config's `[tmdb] api_key`.
    #[arg(long, env = "IRIS_TMDB__API_KEY", hide_env_values = true)]
    tmdb_key: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = iris_config::AppConfig::load(&cli.config).ok();
    let db = match (cli.db, cfg.as_ref()) {
        (Some(p), _) => p,
        (None, Some(c)) => c.storage.data_dir.join("iris.db"),
        (None, None) => anyhow::bail!(
            "no --db given and config {} unreadable",
            cli.config.display()
        ),
    };
    anyhow::ensure!(db.exists(), "database {} not found", db.display());
    let key = cli
        .tmdb_key
        .or_else(|| cfg.and_then(|c| c.tmdb).map(|t| t.api_key))
        .context("no TMDB key: pass --tmdb-key or set [tmdb] api_key")?;
    let pool = iris_db::connect(&db)
        .await
        .with_context(|| format!("opening {}", db.display()))?;
    iris_db::migrate::run(&pool).await.context("migrating")?;
    let tmdb = iris_api::tmdb::TmdbClient::new(key)?;
    let report = iris_api::tmdb_trust_audit::run(&pool, &tmdb, cli.apply).await?;
    print!("{}", iris_api::tmdb_trust_audit::render(&report));
    pool.close().await;
    Ok(())
}
