use std::{num::NonZeroU32, time::Duration};

use async_trait::async_trait;
use url::Url;

use crate::{ChapterInfo, ChapterKey, Image, Result, Series, SeriesHit, SeriesKey, SourceError};

/// How many requests a source tolerates in a time window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    pub requests: NonZeroU32,
    pub per: Duration,
}

impl RateLimit {
    #[must_use]
    pub const fn per_second(requests: u32) -> Self {
        Self::new(requests, Duration::from_secs(1))
    }

    #[must_use]
    pub const fn per_minute(requests: u32) -> Self {
        Self::new(requests, Duration::from_secs(60))
    }

    /// A limit of zero requests is treated as one.
    #[must_use]
    pub const fn new(requests: u32, per: Duration) -> Self {
        let requests = match NonZeroU32::new(requests) {
            Some(n) => n,
            None => NonZeroU32::MIN,
        };
        Self { requests, per }
    }
}

/// Static description of a source.
#[derive(Debug, Clone)]
pub struct SourceMeta {
    /// Stable identifier stored in the database, e.g. `"weebcentral"`.
    pub id: &'static str,
    /// Human readable name, e.g. `"WeebCentral"`.
    pub name: &'static str,
    pub base_url: Url,
    /// Languages the source offers, as normalized BCP 47 tags (`en`, `pt-BR`).
    pub languages: &'static [&'static str],
    pub rate_limit: RateLimit,
}

impl SourceMeta {
    /// The first language listed: what a search without a language uses.
    #[must_use]
    pub fn main_language(&self) -> &'static str {
        self.languages.first().copied().unwrap_or("en")
    }
}

/// What a pasted URL points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlTarget {
    Series(SeriesKey),
    Chapter(ChapterKey),
}

/// A manga site kaze can download from.
///
/// `async_trait` boxes the returned futures so the trait stays
/// object-safe: the app keeps sources as `Arc<dyn Source>` in a registry.
#[async_trait]
pub trait Source: Send + Sync + 'static {
    fn meta(&self) -> &SourceMeta;

    /// Recognise a URL from this site, e.g. one pasted into "Add series".
    fn parse_url(&self, url: &Url) -> Option<UrlTarget>;

    // `language` is one of `meta().languages`; sources with only one
    // language can ignore it.

    async fn search(&self, query: &str, language: &str) -> Result<Vec<SeriesHit>>;

    async fn series(&self, key: &SeriesKey, language: &str) -> Result<Series>;

    /// All chapters of a series in `language`, in any order.
    async fn chapters(&self, key: &SeriesKey, language: &str) -> Result<Vec<ChapterInfo>>;

    /// Image URLs of a chapter's pages, in reading order.
    async fn pages(&self, chapter: &ChapterKey) -> Result<Vec<Url>>;

    /// Download one page image. Lives on the source because image hosts
    /// often need their own referer or rate limit.
    async fn fetch_image(&self, url: &Url) -> Result<Image>;

    /// Series updated recently, from the site's "latest updates" page.
    /// Lets kaze notice new chapters with one request instead of one per
    /// followed series.
    async fn latest_updates(&self) -> Result<Vec<SeriesKey>> {
        Err(SourceError::Unsupported("latest updates"))
    }
}
