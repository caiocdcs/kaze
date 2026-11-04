mod cli;

use clap::Parser;
use kaze::logs::RecentLogs;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _};

/// Log lines kept for Settings → System → Logs.
const RECENT_LOGS: usize = 2000;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_env("KAZE_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| "kaze=info".into());
    let recent = RecentLogs::new(RECENT_LOGS);
    // The filter is a layer of its own, so it applies to both outputs.
    let logs = tracing_subscriber::registry()
        .with(filter)
        .with(recent.layer());
    let stderr = fmt::layer().with_writer(std::io::stderr); // keep stdout clean for `--json`
    if std::env::var("KAZE_LOG_FORMAT").is_ok_and(|f| f.eq_ignore_ascii_case("json")) {
        logs.with(stderr.json()).init();
    } else {
        logs.with(stderr).init();
    }

    cli::Cli::parse().run(recent).await
}
