use std::path::PathBuf;

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
    /// Holds the write lock throughout: run it with the server stopped.
    Vacuum {
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
    }
}
