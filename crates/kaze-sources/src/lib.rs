//! Site implementations of [`kaze_core::Source`], one module per site,
//! each behind its own cargo feature, plus the [`http`] client they share.

#[cfg(feature = "scraping")]
mod html;
pub mod http;
#[cfg(feature = "weebcentral")]
pub mod weebcentral;

use std::sync::Arc;

use kaze_core::{Source, UrlTarget};
use url::Url;

use crate::http::Http;

/// All sources compiled into this build, looked up by id or URL.
#[derive(Clone)]
pub struct Registry {
    sources: Vec<Arc<dyn Source>>,
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
        ];
        let _ = http; // unused when built without any source feature
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
}
