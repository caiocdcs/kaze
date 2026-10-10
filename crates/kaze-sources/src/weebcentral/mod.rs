//! [WeebCentral](https://weebcentral.com): HTML pages rendered for HTMX,
//! no JavaScript needed. Chapter images come from an `/images` fragment
//! the reader page would otherwise load with HTMX.

mod parse;

use async_trait::async_trait;
use kaze_core::{
    ChapterInfo, ChapterKey, Image, RateLimit, Result, Series, SeriesHit, SeriesKey, Source,
    SourceMeta, UrlTarget,
};
use url::Url;

use crate::{
    http::{Http, SourceHttp},
    literal::url,
};

const BASE_URL: &str = "https://weebcentral.com";

pub struct WeebCentral {
    meta: SourceMeta,
    /// HTML pages on weebcentral.com: be gentle.
    site: SourceHttp,
    /// Page images on their CDN: a separate, faster budget.
    images: SourceHttp,
}

impl WeebCentral {
    #[must_use]
    pub fn new(http: &Http) -> Self {
        Self::with_base_url(http, url!(BASE_URL))
    }

    /// Point at another host; tests use this with a mock server.
    #[must_use]
    pub fn with_base_url(http: &Http, base_url: Url) -> Self {
        let rate_limit = RateLimit::per_second(2);
        Self {
            site: http.for_source(rate_limit, Some(&base_url)),
            images: http.for_source(RateLimit::per_second(8), Some(&base_url)),
            meta: SourceMeta {
                id: "weebcentral",
                name: "WeebCentral",
                base_url,
                languages: &["en"],
                rate_limit,
            },
        }
    }

    fn url(&self, path: &str) -> Url {
        let mut url = self.meta.base_url.clone();
        url.set_path(path);
        url
    }
}

#[async_trait]
impl Source for WeebCentral {
    fn meta(&self) -> &SourceMeta {
        &self.meta
    }

    fn parse_url(&self, url: &Url) -> Option<UrlTarget> {
        let host = url.host_str()?.trim_start_matches("www.");
        if host != self.meta.base_url.host_str()?.trim_start_matches("www.") {
            return None;
        }
        let mut segments = url.path_segments()?;
        match (segments.next()?, segments.next()?) {
            ("series", id) if parse::is_ulid(id) => Some(UrlTarget::Series(id.into())),
            ("chapters", id) if parse::is_ulid(id) => Some(UrlTarget::Chapter(id.into())),
            _ => None,
        }
    }

    async fn search(&self, query: &str) -> Result<Vec<SeriesHit>> {
        let mut url = self.url("/search/data");
        url.query_pairs_mut()
            .append_pair("limit", "32")
            .append_pair("offset", "0")
            .append_pair("text", query)
            .append_pair("sort", "Best Match")
            .append_pair("order", "Ascending")
            .append_pair("official", "Any")
            .append_pair("display_mode", "Full Display");
        let html = self.site.get_text(&url).await?;
        parse::search(&html, &self.meta.base_url)
    }

    async fn series(&self, key: &SeriesKey) -> Result<Series> {
        let url = self.url(&format!("/series/{key}"));
        let html = self.site.get_text(&url).await?;
        parse::series(&html, key, &url)
    }

    async fn chapters(&self, key: &SeriesKey) -> Result<Vec<ChapterInfo>> {
        let url = self.url(&format!("/series/{key}/full-chapter-list"));
        let html = self.site.get_text(&url).await?;
        parse::chapters(&html, &self.meta.base_url)
    }

    async fn pages(&self, chapter: &ChapterKey) -> Result<Vec<Url>> {
        let mut url = self.url(&format!("/chapters/{chapter}/images"));
        url.query_pairs_mut()
            .append_pair("is_prev", "False")
            .append_pair("current_page", "1")
            .append_pair("reading_style", "long_strip");
        let html = self.site.get_text(&url).await?;
        parse::pages(&html)
    }

    async fn fetch_image(&self, url: &Url) -> Result<Image> {
        self.images.get_image(url).await
    }

    async fn latest_updates(&self) -> Result<Vec<SeriesKey>> {
        let html = self.site.get_text(&self.url("/latest-updates/1")).await?;
        Ok(parse::latest_updates(&html))
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

    const ONE_PIECE: &str = "01J76XY7E9FNDZ1DBBM6PBJPFK";

    fn source(base: &str) -> WeebCentral {
        WeebCentral::with_base_url(
            &Http::new(HttpConfig::default()).unwrap(),
            base.parse().unwrap(),
        )
    }

    #[test]
    fn recognises_urls() {
        let wc = source(BASE_URL);
        let parse = |u: &str| wc.parse_url(&u.parse().unwrap());
        assert_eq!(
            parse("https://weebcentral.com/series/01J76XY7E9FNDZ1DBBM6PBJPFK/One-Piece"),
            Some(UrlTarget::Series(ONE_PIECE.into()))
        );
        assert_eq!(
            parse("https://www.weebcentral.com/chapters/01M4FFVTBPS4RKXQWZ2REYGH3M"),
            Some(UrlTarget::Chapter("01M4FFVTBPS4RKXQWZ2REYGH3M".into()))
        );
        assert_eq!(parse("https://weebcentral.com/search?text=x"), None);
        assert_eq!(parse("https://weebcentral.com/series/not-an-id"), None);
        assert_eq!(
            parse("https://mangadex.org/series/01J76XY7E9FNDZ1DBBM6PBJPFK"),
            None
        );
    }

    #[tokio::test]
    async fn pages_requests_the_images_fragment() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/chapters/01M4FFVTBPS4RKXQWZ2REYGH3M/images"))
            .and(query_param("reading_style", "long_strip"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(include_str!("fixtures/images.html")),
            )
            .expect(1)
            .mount(&server)
            .await;

        let pages = source(&server.uri())
            .pages(&"01M4FFVTBPS4RKXQWZ2REYGH3M".into())
            .await
            .unwrap();
        assert_eq!(pages.len(), 16);
    }

    #[tokio::test]
    async fn search_sends_query_text() {
        let server = MockServer::start().await;
        Mock::given(path("/search/data"))
            .and(query_param("text", "one piece"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(include_str!("fixtures/search.html")),
            )
            .expect(1)
            .mount(&server)
            .await;

        let hits = source(&server.uri()).search("one piece").await.unwrap();
        assert_eq!(hits[0].key.as_str(), ONE_PIECE);
    }

    /// Hits the real site: `just test-live`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_one_piece() {
        let wc = source(BASE_URL);
        let hits = wc.search("one piece").await.unwrap();
        assert!(hits.iter().any(|h| h.key.as_str() == ONE_PIECE));

        let series = wc.series(&ONE_PIECE.into()).await.unwrap();
        assert_eq!(series.title, "One Piece");

        let chapters = wc.chapters(&ONE_PIECE.into()).await.unwrap();
        assert!(chapters.len() > 1000);

        let newest = chapters
            .iter()
            .max_by(|a, b| a.number.cmp(&b.number))
            .unwrap();
        let pages = wc.pages(&newest.key).await.unwrap();
        assert_ne!(pages.len(), 0);
        let image = wc.fetch_image(&pages[0]).await.unwrap();
        assert!(image.bytes.len() > 10_000);

        assert_ne!(wc.latest_updates().await.unwrap().len(), 0);
    }
}
