//! Site implementations of [`kaze_core::Source`], one module per site,
//! each behind its own cargo feature, plus the [`http`] client they share.

#[cfg(feature = "scraping")]
mod html;
pub mod http;
#[cfg(feature = "mangadex")]
pub mod mangadex;
#[cfg(feature = "weebcentral")]
pub mod weebcentral;

use std::{sync::Arc, time::Duration};

use kaze_core::{Result, SeriesHit, Source, SourceError, UrlTarget};
use tokio::task::JoinSet;
use url::Url;

use crate::http::Http;

/// What one source answered to a global search.
#[derive(Debug)]
pub struct SearchOutcome {
    pub source: &'static str,
    pub result: Result<Vec<SeriesHit>>,
}

/// All sources compiled into this build, looked up by id or URL.
#[derive(Clone)]
pub struct Registry {
    sources: Vec<Arc<dyn Source>>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.sources.iter().map(|s| s.meta().id))
            .finish()
    }
}

impl Registry {
    /// Every source enabled by cargo features.
    #[must_use]
    pub fn new(http: &Http) -> Self {
        // `#[cfg]` works on individual elements, so a disabled feature just
        // drops its line from the list.
        let sources: Vec<Arc<dyn Source>> = vec![
            #[cfg(feature = "weebcentral")]
            Arc::new(weebcentral::WeebCentral::new(http)),
            #[cfg(feature = "mangadex")]
            Arc::new(mangadex::MangaDex::new(http)),
        ];
        let _ = http; // unused when built without any source feature
        Self { sources }
    }

    /// A registry of exactly these sources (handy in tests).
    #[must_use]
    pub fn from_sources(sources: Vec<Arc<dyn Source>>) -> Self {
        Self { sources }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Source>> {
        self.sources.iter()
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Arc<dyn Source>> {
        self.sources.iter().find(|s| s.meta().id == id)
    }

    /// The source that recognises `url`, and what the URL points to.
    #[must_use]
    pub fn resolve(&self, url: &Url) -> Option<(&Arc<dyn Source>, UrlTarget)> {
        self.sources
            .iter()
            .find_map(|s| s.parse_url(url).map(|target| (s, target)))
    }

    /// Search every source concurrently. A slow or failing source only
    /// affects its own entry; results come back in registry order.
    pub async fn search_all(&self, query: &str, timeout: Duration) -> Vec<SearchOutcome> {
        let mut tasks = JoinSet::new();
        for (index, source) in self.sources.iter().enumerate() {
            // Spawned tasks must be 'static: give each its own handles.
            let source = Arc::clone(source);
            let query = query.to_owned();
            tasks.spawn(async move {
                let result = tokio::time::timeout(timeout, source.search(&query))
                    .await
                    .unwrap_or(Err(SourceError::Timeout(timeout)));
                (
                    index,
                    SearchOutcome {
                        source: source.meta().id,
                        result,
                    },
                )
            });
        }

        let mut outcomes = Vec::with_capacity(self.sources.len());
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok(outcome) => outcomes.push(outcome),
                Err(err) => tracing::error!("search task panicked: {err}"),
            }
        }
        outcomes.sort_by_key(|(index, _)| *index);
        outcomes.into_iter().map(|(_, outcome)| outcome).collect()
    }
}
