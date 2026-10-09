//! Command line interface. Besides `serve`, the commands talk to sources
//! directly, which is handy for trying out a source without the UI.

use std::{fmt::Write as _, time::Duration};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use kaze_core::{ChapterInfo, Series, SeriesHit, Source, UrlTarget};
use kaze_sources::{
    Registry,
    http::{Http, HttpConfig},
};
use url::Url;

/// Light, self-hosted manga downloader.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the server (API, UI and download workers).
    Serve,

    /// List the sources built into this binary.
    Sources,

    /// Search for a series on every source (or one with --source).
    Search {
        /// What to search for.
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        /// Only search this source, e.g. `weebcentral`.
        #[arg(long, short)]
        source: Option<String>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Show what a series or chapter URL points to.
    Info {
        url: Url,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

impl Cli {
    pub async fn run(self) -> anyhow::Result<()> {
        let http = Http::new(HttpConfig::default()).context("building HTTP client")?;
        let registry = Registry::new(&http);

        match self.command {
            Command::Serve => {
                tracing::info!("kaze {} — nothing to serve yet", env!("CARGO_PKG_VERSION"));
            }
            Command::Sources => {
                for source in registry.iter() {
                    let meta = source.meta();
                    println!("{:<12} {:<14} {}", meta.id, meta.name, meta.base_url);
                }
            }
            Command::Search {
                query,
                source,
                json,
            } => {
                search(&registry, &query.join(" "), source.as_deref(), json).await?;
            }
            Command::Info { url, json } => info(&registry, &url, json).await?,
        }
        Ok(())
    }
}

async fn search(
    registry: &Registry,
    query: &str,
    only: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let outcomes = match only {
        Some(id) => {
            let source = registry
                .get(id)
                .with_context(|| format!("unknown source {id:?}"))?;
            let hits = source.search(query).await?;
            vec![(id.to_owned(), hits)]
        }
        None => registry
            .search_all(query, Duration::from_secs(20))
            .await
            .into_iter()
            .filter_map(|outcome| match outcome.result {
                Ok(hits) => Some((outcome.source.to_owned(), hits)),
                Err(err) => {
                    tracing::warn!(source = outcome.source, "search failed: {err}");
                    None
                }
            })
            .collect(),
    };

    if json {
        let value: serde_json::Map<_, _> = outcomes
            .into_iter()
            .map(|(id, hits)| (id, serde_json::to_value(hits).expect("hits serialize")))
            .collect();
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    for (id, hits) in &outcomes {
        println!("{id} ({} results)", hits.len());
        for SeriesHit { title, url, .. } in hits {
            println!("  {title}\n    {url}");
        }
    }
    Ok(())
}

async fn info(registry: &Registry, url: &Url, json: bool) -> anyhow::Result<()> {
    let Some((source, target)) = registry.resolve(url) else {
        bail!("no source recognises {url}");
    };
    match target {
        UrlTarget::Series(key) => {
            // Metadata and chapter list are independent: fetch both at once.
            let (series, mut chapters) =
                tokio::try_join!(source.series(&key), source.chapters(&key))?;
            chapters.sort_by(|a, b| a.number.cmp(&b.number));
            if json {
                let value = serde_json::json!({ "series": series, "chapters": chapters });
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                print!("{}", describe(source.as_ref(), &series, &chapters));
            }
        }
        UrlTarget::Chapter(key) => {
            let pages = source.pages(&key).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&pages)?);
            } else {
                println!("{} pages", pages.len());
                for page in pages {
                    println!("  {page}");
                }
            }
        }
    }
    Ok(())
}

fn describe(source: &dyn Source, series: &Series, chapters: &[ChapterInfo]) -> String {
    let mut out = String::new();
    // Writing to a String cannot fail, hence the ignored results.
    let _ = writeln!(out, "{} [{}]", series.title, source.meta().name);
    let _ = writeln!(out, "  url:      {}", series.url);
    if !series.authors.is_empty() {
        let _ = writeln!(out, "  authors:  {}", series.authors.join(", "));
    }
    let _ = writeln!(out, "  status:   {:?}", series.status);
    if let Some(year) = series.year {
        let _ = writeln!(out, "  year:     {year}");
    }
    if !series.tags.is_empty() {
        let _ = writeln!(out, "  tags:     {}", series.tags.join(", "));
    }
    let _ = write!(out, "  chapters: {}", chapters.len());
    if let (Some(first), Some(last)) = (chapters.first(), chapters.last()) {
        let _ = write!(out, " ({} – {})", first.number, last.number);
    }
    out.push('\n');
    out
}
