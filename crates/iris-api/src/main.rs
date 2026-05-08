use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "iris", version, about = "Iris API server")]
struct Cli {
    /// Path to config.toml
    #[arg(short, long, env = "IRIS_CONFIG", default_value = "config/config.toml")]
    config: PathBuf,

    /// Path to providers.toml (overrides config-specified path if set)
    #[arg(long, env = "IRIS_PROVIDERS")]
    providers: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    iris_api::observability::init_tracing();
    let cli = Cli::parse();
    iris_api::run(cli.config, cli.providers).await
}
