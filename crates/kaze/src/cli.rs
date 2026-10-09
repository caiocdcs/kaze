//! Command line interface. Besides `serve`, the commands talk to sources
//! directly, which is handy for trying out a source without the UI.

use std::{fmt::Write as _, path::PathBuf, time::Duration};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use kaze::download::{
    self, ChapterMeta, Outcome,
    naming::{Template, sanitize_file_name},
    select::Selection,
};
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

    /// Download chapters of a series to CBZ files, without the server.
    Download {
        /// Series URL, e.g. https://weebcentral.com/series/<id>/<name>
        url: Url,
        /// Which chapters: `5`, `1-10`, `1-3,7`, `latest` or `all`.
        #[arg(long, short)]
        chapters: Selection,
        /// Library folder; the series gets a sub-folder inside it.
        #[arg(long, short, default_value = "downloads")]
        out: PathBuf,
        /// File name template (see `download::naming`).
        #[arg(long, default_value = download::naming::DEFAULT_TEMPLATE)]
        template: Template,
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
            Command::Download {
                url,
                chapters,
                out,
                template,
            } => download(&registry, &url, &chapters, &out, template).await?,
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

async fn download(
    registry: &Registry,
    url: &Url,
    selection: &Selection,
    library: &std::path::Path,
    template: Template,
) -> anyhow::Result<()> {
    let Some((source, UrlTarget::Series(key))) = registry.resolve(url) else {
        bail!("{url} is not a series URL of a known source");
    };
    let (series, chapters) = tokio::try_join!(source.series(&key), source.chapters(&key))?;
    let mut selected = selection.filter(&chapters, |c| &c.number);
    selected.sort_by(|a, b| a.number.cmp(&b.number));
    if selected.is_empty() {
        bail!("no chapters of {} match the selection", series.title);
    }

    let dir = library.join(sanitize_file_name(&series.title));
    println!(
        "{}: {} chapters → {}",
        series.title,
        selected.len(),
        dir.display()
    );
    if let Some(cover) = &series.cover_url
        && let Err(err) = download::cover(source.as_ref(), cover, &dir).await
    {
        tracing::warn!("could not save cover: {err}");
    }

    let options = download::Options {
        template,
        ..download::Options::default()
    };
    let mut failed = 0;
    for chapter in selected {
        let meta = ChapterMeta::new(&series, chapter);
        let label = format!("Ch.{}", chapter.number);
        let progress = |done: usize, total: usize| tracing::debug!("{label}: page {done}/{total}");
        match download::chapter(
            source.as_ref(),
            &chapter.key,
            &meta,
            &dir,
            &options,
            &progress,
        )
        .await
        {
            Ok(Outcome::Downloaded { path, pages }) => {
                println!("  {label:<10} {pages:>3} pages  {}", file_name(&path));
            }
            Ok(Outcome::AlreadyExists(path)) => {
                println!("  {label:<10}   exists   {}", file_name(&path));
            }
            Err(err) => {
                failed += 1;
                println!("  {label:<10}   FAILED   {err}");
            }
        }
    }
    if failed > 0 {
        bail!("{failed} chapter(s) failed");
    }
    Ok(())
}

fn file_name(path: &std::path::Path) -> std::borrow::Cow<'_, str> {
    path.file_name().unwrap_or_default().to_string_lossy()
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
