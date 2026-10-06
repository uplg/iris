//! What the filename-parser fix does to the library's collections, without
//! writing anything: `cargo run -p iris-api --bin parse-dryrun -- --db <path>`.
//! Point it at a copy (`VACUUM INTO`) of the live database: the copy is
//! migrated to the current schema before the read.

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "parse-dryrun",
    about = "Collection identity changes from the filename-parser fix (read only)"
)]
struct Cli {
    /// `SQLite` database to read (a copy: it gets migrated).
    #[arg(long)]
    db: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    anyhow::ensure!(cli.db.exists(), "database {} not found", cli.db.display());
    let pool = iris_db::connect(&cli.db)
        .await
        .with_context(|| format!("opening {}", cli.db.display()))?;
    iris_db::migrate::run(&pool).await.context("migrating")?;
    let report = iris_api::parse_audit::run(&pool).await?;
    print!("{}", iris_api::parse_audit::render(&report));
    pool.close().await;
    Ok(())
}
