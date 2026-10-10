//! Test doubles shared by unit tests.

use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use bytes::Bytes;
use kaze_core::{
    ChapterInfo, ChapterKey, Image, RateLimit, Result, Series, SeriesHit, SeriesKey, SeriesStatus,
    Source, SourceError, SourceMeta, UrlTarget,
};
use url::Url;

pub const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

/// An in-memory source: one series, configurable chapters and pages, and
/// a switch to make the next image downloads fail.
pub struct FakeSource {
    meta: SourceMeta,
    pub series: Series,
    pub chapters: Mutex<Vec<ChapterInfo>>,
    pub pages: Vec<&'static [u8]>,
    pub latest: Mutex<Vec<SeriesKey>>,
    /// The next N `fetch_image` calls fail with a retryable 503.
    pub fail_images: AtomicUsize,
    pub image_fetches: AtomicUsize,
}

impl FakeSource {
    pub fn new() -> Self {
        let base = Url::parse("https://fake.test").unwrap();
        Self {
            series: Series {
                key: "S1".into(),
                title: "Test Series".into(),
                alt_titles: vec![],
                url: base.join("/series/S1").unwrap(),
                cover_url: Some(base.join("/cover/0").unwrap()),
                description: None,
                authors: vec!["Author".into()],
                tags: vec![],
                status: SeriesStatus::Ongoing,
                year: Some(2020),
                language: Some("en".into()),
            },
            meta: SourceMeta {
                id: "fake",
                name: "Fake",
                base_url: base,
                languages: &["en"],
                rate_limit: RateLimit::per_second(1000),
            },
            chapters: Mutex::new(Vec::new()),
            pages: vec![JPEG, PNG],
            latest: Mutex::new(Vec::new()),
            fail_images: AtomicUsize::new(0),
            image_fetches: AtomicUsize::new(0),
        }
    }

    #[must_use]
    pub fn with_pages(mut self, pages: &[&'static [u8]]) -> Self {
        self.pages = pages.to_vec();
        self
    }

    #[must_use]
    pub fn with_chapters(self, numbers: &[&str]) -> Self {
        for n in numbers {
            self.add_chapter(n);
        }
        self
    }

    pub fn add_chapter(&self, number: &str) {
        self.chapters.lock().unwrap().push(ChapterInfo {
            key: format!("C{number}").into(),
            number: number.parse().unwrap(),
            volume: None,
            title: None,
            url: self
                .meta
                .base_url
                .join(&format!("/chapters/C{number}"))
                .unwrap(),
            language: Some("en".into()),
            scanlator: None,
            published_at: None,
        });
    }

    pub fn fetches(&self) -> usize {
        self.image_fetches.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Source for FakeSource {
    fn meta(&self) -> &SourceMeta {
        &self.meta
    }

    fn parse_url(&self, _: &Url) -> Option<UrlTarget> {
        None
    }

    async fn search(&self, _: &str) -> Result<Vec<SeriesHit>> {
        Ok(vec![SeriesHit {
            key: self.series.key.clone(),
            title: self.series.title.clone(),
            url: self.series.url.clone(),
            cover_url: self.series.cover_url.clone(),
        }])
    }

    async fn series(&self, _: &SeriesKey) -> Result<Series> {
        Ok(self.series.clone())
    }

    async fn chapters(&self, _: &SeriesKey) -> Result<Vec<ChapterInfo>> {
        Ok(self.chapters.lock().unwrap().clone())
    }

    async fn pages(&self, chapter: &ChapterKey) -> Result<Vec<Url>> {
        Ok((0..self.pages.len())
            .map(|i| self.meta.base_url.join(&format!("/{chapter}/{i}")).unwrap())
            .collect())
    }

    async fn fetch_image(&self, url: &Url) -> Result<Image> {
        self.image_fetches.fetch_add(1, Ordering::SeqCst);
        // Decrement only if positive: a compare-and-swap loop in one call.
        let failing = self
            .fail_images
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if failing {
            return Err(SourceError::Status {
                url: url.clone(),
                status: 503,
            });
        }
        let index: usize = url
            .path_segments()
            .unwrap()
            .next_back()
            .unwrap()
            .parse()
            .unwrap();
        Ok(Image {
            bytes: Bytes::from_static(self.pages[index]),
            content_type: None,
        })
    }

    async fn latest_updates(&self) -> Result<Vec<SeriesKey>> {
        Ok(self.latest.lock().unwrap().clone())
    }
}
