//! Command line interface. Besides `serve`, the commands talk to sources
//! directly, which is handy for trying out a source without the UI.

use std::{fmt::Write as _, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context as _, bail};
use clap::{Parser, Subcommand};
use kaze::{
    api,
    app::{App, Context},
    config::Config,
    db,
    download::{
        self, ChapterMeta, Outcome,
        naming::{Template, sanitize_file_name},
        select::Selection,
    },
    events::Events,
    jobs::RetryPolicy,
    settings::SettingsStore,
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
    Serve {
        /// Config file (TOML). Settings can also come from KAZE_* variables.
        #[arg(long, short, env = "KAZE_CONFIG")]
        config: Option<PathBuf>,
    },

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
        /// Language, e.g. `pt-BR`; defaults to the source's main one.
        #[arg(long, short, value_parser = parse_language)]
        language: Option<String>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Show what a series or chapter URL points to.
    Info {
        url: Url,
        /// Language, e.g. `pt-BR`; defaults to the source's main one.
        #[arg(long, short, value_parser = parse_language)]
        language: Option<String>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Download chapters of a series to CBZ files, without the server.
    Download {
        /// Series URL, e.g. https://weebcentral.com/series/<id>/<name>
        url: Url,
        /// Language, e.g. `pt-BR`; defaults to the source's main one.
        #[arg(long, short, value_parser = parse_language)]
        language: Option<String>,
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
        if let Command::Serve { config } = &self.command {
            return serve(config.as_deref()).await;
        }
        let http = Http::new(HttpConfig::default()).context("building HTTP client")?;
        let registry = Registry::new(&http);

        match self.command {
            Command::Serve { .. } => unreachable!("handled above"),
            Command::Sources => {
                for source in registry.iter() {
                    let meta = source.meta();
                    println!("{:<12} {:<14} {}", meta.id, meta.name, meta.base_url);
                }
            }
            Command::Search {
                query,
                source,
                language,
                json,
            } => {
                let query = query.join(" ");
                search(
                    &registry,
                    &query,
                    source.as_deref(),
                    language.as_deref(),
                    json,
                )
                .await?;
            }
            Command::Info {
                url,
                language,
                json,
            } => info(&registry, &url, language.as_deref(), json).await?,
            Command::Download {
                url,
                language,
                chapters,
                out,
                template,
            } => {
                let language = language.as_deref();
                download(&registry, &url, language, &chapters, &out, template).await?;
            }
        }
        Ok(())
    }
}

async fn serve(config_file: Option<&std::path::Path>) -> anyhow::Result<()> {
    let config = Config::load(config_file).context("reading configuration")?;
    for dir in [&config.data_dir, &config.library_dir] {
        tokio::fs::create_dir_all(dir)
            .await
            .with_context(|| format!("creating {}", dir.display()))?;
    }

    let db = db::connect(&config.database_path())
        .await
        .context("opening database")?;
    let http = Http::new(config.http()).context("building HTTP client")?;
    let settings = SettingsStore::load(db.clone(), config.settings.clone())
        .await
        .context("invalid [settings] in the configuration")?;
    let ctx = Context {
        db,
        sources: Registry::new(&http),
        events: Events::new(1024),
        library: config.library_dir.clone(),
        settings,
        http: http.client(),
        flaresolverr: http.flaresolverr(),
    };
    let app = Arc::new(
        App::start(ctx, RetryPolicy::default())
            .await
            .context("starting jobs")?,
    );

    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("listening on {}", config.listen))?;
    tracing::info!(
        "kaze {} listening on http://{}, library at {}",
        env!("CARGO_PKG_VERSION"),
        config.listen,
        config.library_dir.display()
    );
    log_setup(&app).await;

    let shutdown = app.shutdown_token();
    axum::serve(listener, api::router(Arc::clone(&app)))
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            tracing::info!("shutting down");
            // Ends SSE streams so the server can finish draining.
            shutdown.cancel();
        })
        .await?;
    app.shutdown().await;
    Ok(())
}

async fn log_setup(app: &App) {
    let ctx = &app.ctx;
    let sources: Vec<&str> = ctx.sources.iter().map(|s| s.meta().id).collect();
    let settings = ctx.settings.current();
    let integrations = db::integrations::list(&ctx.db).await.unwrap_or_default();
    let integrations: Vec<String> = integrations
        .iter()
        .map(|i| {
            let state = if i.enabled { "" } else { " (disabled)" };
            format!("{} [{}]{state}", i.name, i.config.0.kind())
        })
        .collect();
    tracing::info!(
        sources = sources.join(", "),
        flaresolverr = settings
            .flaresolverr_url
            .as_ref()
            .map_or("off", Url::as_str),
        integrations = if integrations.is_empty() {
            "none".to_owned()
        } else {
            integrations.join(", ")
        },
        "setup"
    );
}

/// Ctrl-C, or SIGTERM from `docker stop` / systemd.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

fn parse_language(tag: &str) -> Result<String, String> {
    kaze_core::language::normalize(tag).ok_or_else(|| format!("not a language tag: {tag:?}"))
}

/// `wanted` if the source offers it, else its main language.
fn language_of<'a>(source: &dyn Source, wanted: Option<&'a str>) -> anyhow::Result<&'a str> {
    let meta = source.meta();
    match wanted {
        Some(l) if meta.languages.contains(&l) => Ok(l),
        Some(l) => bail!(
            "{} has no {l}; it offers {}",
            meta.name,
            meta.languages.join(", ")
        ),
        None => Ok(meta.main_language()),
    }
}

async fn search(
    registry: &Registry,
    query: &str,
    only: Option<&str>,
    language: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let outcomes = match only {
        Some(id) => {
            let source = registry
                .get(id)
                .with_context(|| format!("unknown source {id:?}"))?;
            let language = language_of(source.as_ref(), language)?;
            let hits = source.search(query, language).await?;
            vec![(id.to_owned(), hits)]
        }
        None => registry
            .search_all(query, language, Duration::from_secs(20))
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
        let value = outcomes
            .into_iter()
            .map(|(id, hits)| Ok((id, serde_json::to_value(hits)?)))
            .collect::<serde_json::Result<serde_json::Map<_, _>>>()?;
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

async fn info(
    registry: &Registry,
    url: &Url,
    language: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let Some((source, target)) = registry.resolve(url) else {
        bail!("no source recognises {url}");
    };
    match target {
        UrlTarget::Series(key) => {
            let language = language_of(source.as_ref(), language)?;
            // Metadata and chapter list are independent: fetch both at once.
            let (series, mut chapters) = tokio::try_join!(
                source.series(&key, language),
                source.chapters(&key, language)
            )?;
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
    language: Option<&str>,
    selection: &Selection,
    library: &std::path::Path,
    template: Template,
) -> anyhow::Result<()> {
    let Some((source, UrlTarget::Series(key))) = registry.resolve(url) else {
        bail!("{url} is not a series URL of a known source");
    };
    let language = language_of(source.as_ref(), language)?;
    let (series, chapters) = tokio::try_join!(
        source.series(&key, language),
        source.chapters(&key, language)
    )?;
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
