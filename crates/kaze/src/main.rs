use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

/// Light, self-hosted manga downloader.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the server (API, UI and download workers).
    Serve,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "kaze=info".into()))
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Serve => {
            tracing::info!("kaze {} — nothing to serve yet", env!("CARGO_PKG_VERSION"));
        }
    }
}
