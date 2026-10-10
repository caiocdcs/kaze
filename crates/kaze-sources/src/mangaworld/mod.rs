//! [Mangaworld](https://www.mangaworld.mx): Italian scans, plain HTML.
//! The reader's `?style=list` view lists every page image at once.

mod parse;

use async_trait::async_trait;
use kaze_core::{
    ChapterInfo, ChapterKey, Image, RateLimit, Result, Series, SeriesHit, SeriesKey, Source,
    SourceError, SourceMeta, UrlTarget,
};
use url::Url;

use crate::http::{Http, SourceHttp};

const BASE_URL: &str = "https://www.mangaworld.mx";

pub struct Mangaworld {
    meta: SourceMeta,
    site: SourceHttp,
    images: SourceHttp,
}

impl Mangaworld {
    #[must_use]
    pub fn new(http: &Http) -> Self {
        Self::with_base_url(http, Url::parse(BASE_URL).expect("valid base url"))
    }

    /// Point at another host; tests use this with a mock server.
    #[must_use]
    pub fn with_base_url(http: &Http, base_url: Url) -> Self {
        let rate_limit = RateLimit::per_second(2);
        Self {
            site: http.for_source(rate_limit, Some(&base_url)),
            images: http.for_source(RateLimit::per_second(8), Some(&base_url)),
            meta: SourceMeta {
                id: "mangaworld",
                name: "Mangaworld",
                base_url,
                languages: &["it"],
                rate_limit,
            },
        }
    }

    fn url(&self, path: &str) -> Url {
        self.meta
            .base_url
            .join(path)
            .expect("relative path joins onto base url")
    }
}

/// The site has moved between TLDs (.so, .cx, .mx, ...); accept any of them.
fn is_mangaworld_host(host: &str) -> bool {
    host.trim_start_matches("www.")
        .split_once('.')
        .is_some_and(|(name, _)| name == "mangaworld")
}

#[async_trait]
impl Source for Mangaworld {
    fn meta(&self) -> &SourceMeta {
        &self.meta
    }

    fn parse_url(&self, url: &Url) -> Option<UrlTarget> {
        let host = url.host_str()?;
        if !is_mangaworld_host(host) && Some(host) != self.meta.base_url.host_str() {
            return None;
        }
        if let Some(key) = parse::chapter_key(url) {
            return Some(UrlTarget::Chapter(key.into()));
        }
        parse::series_key(url).map(|key| UrlTarget::Series(key.into()))
    }

    async fn search(&self, query: &str) -> Result<Vec<SeriesHit>> {
        let mut url = self.url("/archive");
        url.query_pairs_mut().append_pair("keyword", query);
        let html = self.site.get_text(&url).await?;
        Ok(parse::search(&html, &self.meta.base_url))
    }

    async fn series(&self, key: &SeriesKey) -> Result<Series> {
        let url = self.url(&format!("/manga/{key}/"));
        let html = self.site.get_text(&url).await?;
        parse::series(&html, key, &url)
    }

    async fn chapters(&self, key: &SeriesKey) -> Result<Vec<ChapterInfo>> {
        let html = self
            .site
            .get_text(&self.url(&format!("/manga/{key}/")))
            .await?;
        Ok(parse::chapters(&html, &self.meta.base_url))
    }

    async fn pages(&self, chapter: &ChapterKey) -> Result<Vec<Url>> {
        // Keys are "{id}/{slug}/{hash}".
        let (series, hash) = chapter
            .as_str()
            .rsplit_once('/')
            .ok_or_else(|| SourceError::parse("chapter key", format!("unexpected {chapter}")))?;
        let mut url = self.url(&format!("/manga/{series}/read/{hash}"));
        url.query_pairs_mut().append_pair("style", "list");
        let html = self.site.get_text(&url).await?;
        parse::pages(&html, &self.meta.base_url)
    }

    async fn fetch_image(&self, url: &Url) -> Result<Image> {
        self.images.get_image(url).await
    }

    async fn latest_updates(&self) -> Result<Vec<SeriesKey>> {
        let html = self.site.get_text(&self.url("/")).await?;
        Ok(parse::latest(&html, &self.meta.base_url))
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    use super::*;
    use crate::http::HttpConfig;

    fn source(base: &str) -> Mangaworld {
        Mangaworld::with_base_url(
            &Http::new(HttpConfig::default()).unwrap(),
            base.parse().unwrap(),
        )
    }

    fn base() -> Url {
        BASE_URL.parse().unwrap()
    }

    #[test]
    fn recognises_urls_on_any_tld() {
        let mw = source(BASE_URL);
        let parse = |u: &str| mw.parse_url(&u.parse().unwrap());
        assert_eq!(
            parse("https://www.mangaworld.mx/manga/1708/one-piece/"),
            Some(UrlTarget::Series("1708/one-piece".into()))
        );
        assert_eq!(
            parse("https://www.mangaworld.so/manga/1708/one-piece/read/6ac7bc"),
            Some(UrlTarget::Chapter("1708/one-piece/6ac7bc".into()))
        );
        assert_eq!(parse("https://www.mangaworld.mx/archive?keyword=x"), None);
        assert_eq!(parse("https://weebcentral.com/manga/1708/one-piece"), None);
    }

    #[tokio::test]
    async fn search_reads_archive_entries() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/archive"))
            .and(query_param("keyword", "one piece"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(include_str!("fixtures/search.html")),
            )
            .expect(1)
            .mount(&server)
            .await;
        let hits = source(&server.uri()).search("one piece").await.unwrap();
        assert_eq!(hits.len(), 3);
        let one_piece = hits.iter().find(|h| h.title == "One Piece").unwrap();
        assert_eq!(one_piece.key.as_str(), "1708/one-piece");
        assert!(one_piece.cover_url.is_some());
    }

    #[test]
    fn series_metadata() {
        let url = base().join("/manga/1708/one-piece/").unwrap();
        let series = parse::series(
            include_str!("fixtures/series.html"),
            &"1708/one-piece".into(),
            &url,
        )
        .unwrap();
        assert_eq!(series.title, "One Piece");
        assert_eq!(series.authors, ["ODA Eiichiro"]);
        assert_eq!(series.status, kaze_core::SeriesStatus::Ongoing);
        assert_eq!(series.year, Some(1997));
        assert!(series.tags.iter().any(|t| t == "Avventura"));
        assert!(series.alt_titles.iter().any(|t| t == "OP"));
        assert!(series.description.unwrap().starts_with("Monkey D. Rufy"));
        assert_eq!(series.language.as_deref(), Some("it"));
    }

    #[test]
    fn chapters_carry_volumes_and_dates() {
        let chapters = parse::chapters(include_str!("fixtures/series.html"), &base());
        assert_eq!(chapters.len(), 6);
        let newest = &chapters[0];
        assert_eq!(newest.number.to_string(), "1195");
        assert_eq!(newest.volume, Some(116));
        assert_eq!(
            newest.key.as_str(),
            "1708/one-piece/6ac7bcc30f2880415efbf0db"
        );
        assert_eq!(
            newest.published_at.unwrap().to_rfc3339(),
            "2026-10-08T00:00:00+00:00"
        );
        assert_eq!(chapters[3].volume, Some(115));
    }

    #[tokio::test]
    async fn pages_use_the_list_reader() {
        let server = MockServer::start().await;
        Mock::given(path("/manga/1708/one-piece/read/6ac7bc"))
            .and(query_param("style", "list"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(include_str!("fixtures/read.html")),
            )
            .expect(1)
            .mount(&server)
            .await;
        let pages = source(&server.uri())
            .pages(&"1708/one-piece/6ac7bc".into())
            .await
            .unwrap();
        assert_eq!(pages.len(), 4);
        assert!(pages[0].as_str().ends_with("/1.jpg"));
    }

    #[test]
    fn latest_lists_series_from_the_home_page() {
        let keys = parse::latest(include_str!("fixtures/home.html"), &base());
        assert_ne!(keys.len(), 0);
        assert!(keys.iter().all(|k| k.as_str().contains('/')));
    }

    /// Hits the real site: `just test-live`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_one_piece() {
        let mw = Mangaworld::new(&Http::new(HttpConfig::default()).unwrap());
        let hits = mw.search("one piece").await.unwrap();
        let hit = hits.iter().find(|h| h.title == "One Piece").unwrap();
        let series = mw.series(&hit.key).await.unwrap();
        assert_eq!(series.title, "One Piece");
        let chapters = mw.chapters(&hit.key).await.unwrap();
        assert!(chapters.len() > 1000);
        let pages = mw.pages(&chapters[0].key).await.unwrap();
        let image = mw.fetch_image(&pages[0]).await.unwrap();
        assert!(image.bytes.len() > 1000);
        assert_ne!(mw.latest_updates().await.unwrap().len(), 0);
    }
}
