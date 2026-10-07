use std::path::PathBuf;

use anyhow::Context as _;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "iris", version, about = "Iris API server")]
struct Cli {
    /// Path to config.toml
    #[arg(short, long, env = "IRIS_CONFIG", default_value = "config/config.toml")]
    config: PathBuf,

    /// Path to providers.toml (overrides config-specified path if set)
    #[arg(long, env = "IRIS_PROVIDERS")]
    providers: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// One-off upkeep, never run on its own.
    Maintenance {
        #[command(subcommand)]
        task: MaintenanceTask,
    },
}

#[derive(Subcommand, Debug)]
enum MaintenanceTask {
    /// Rebuild the database file to hand back the space a big prune freed.
    /// Holds the write lock throughout: writes from a running server wait,
    /// and fail past the busy timeout.
    Vacuum {
        /// The database (default: `<data_dir>/iris.db` from the config).
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Re-evaluate every collection's TMDB match through the trust gate and
    /// report it; `--apply` writes it. Safe beside a running server.
    TmdbTrust {
        /// Write the result: trusted ids stored with their trust, untrusted
        /// ones cleared, negative and fuzzy resolve-cache entries flushed.
        #[arg(long)]
        apply: bool,
        /// The database (default: `<data_dir>/iris.db` from the config).
        #[arg(long)]
        db: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    iris_api::observability::init_tracing();
    let cli = Cli::parse();
    match cli.command {
        None => iris_api::run(cli.config, cli.providers).await,
        Some(Command::Maintenance {
            task: MaintenanceTask::Vacuum { db },
        }) => {
            let db = match db {
                Some(db) => db,
                None => iris_config::AppConfig::load(&cli.config)?
                    .storage
                    .data_dir
                    .join("iris.db"),
            };
            let (before, after) = iris_api::maintenance::vacuum(&db).await?;
            println!(
                "{}: {before} -> {after} bytes ({} freed)",
                db.display(),
                before.saturating_sub(after)
            );
            Ok(())
        }
        Some(Command::Maintenance {
            task: MaintenanceTask::TmdbTrust { apply, db },
        }) => {
            let cfg = iris_config::AppConfig::load(&cli.config)?;
            let db = db.unwrap_or_else(|| cfg.storage.data_dir.join("iris.db"));
            anyhow::ensure!(db.exists(), "database {} not found", db.display());
            let key = cfg
                .tmdb
                .map(|t| t.api_key)
                .context("no TMDB key: set [tmdb] api_key or IRIS_TMDB__API_KEY")?;
            let pool = iris_db::connect(&db)
                .await
                .with_context(|| format!("opening {}", db.display()))?;
            iris_db::migrate::run(&pool).await.context("migrating")?;
            let tmdb = iris_api::tmdb::TmdbClient::new(key)?;
            let report = iris_api::tmdb_trust_audit::run(&pool, &tmdb, apply).await?;
            print!("{}", iris_api::tmdb_trust_audit::render(&report));
            pool.close().await;
            Ok(())
        }
    }
}
