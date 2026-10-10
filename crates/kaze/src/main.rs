mod cli;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_env("KAZE_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| "kaze=info".into());
    let logs = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr); // keep stdout clean for `--json`
    if std::env::var("KAZE_LOG_FORMAT").is_ok_and(|f| f.eq_ignore_ascii_case("json")) {
        logs.json().init();
    } else {
        logs.init();
    }

    cli::Cli::parse().run().await
}
