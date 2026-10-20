//! [Asura Scans](https://asurascans.com), formerly asuracomic.net. Pages
//! are server-rendered by Astro with the data as island props, so no
//! JavaScript or browser is needed.

mod parse;

use async_trait::async_trait;
use chrono::Utc;
use kaze_core::{
    ChapterInfo, ChapterKey, Image, RateLimit, Result, Series, SeriesHit, SeriesKey, Source,
    SourceMeta, UrlTarget,
};
use url::Url;

use crate::{
    http::{Http, SourceHttp},
    literal::url,
};

const BASE_URL: &str = "https://asurascans.com";

pub struct Asura {
    meta: SourceMeta,
    site: SourceHttp,
    images: SourceHttp,
}

impl Asura {
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
                id: "asura",
                name: "Asura Scans",
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

fn is_asura_host(host: &str) -> bool {
    matches!(
        host.trim_start_matches("www."),
        "asurascans.com" | "asuracomic.net" | "asuratoon.com"
    )
}

#[async_trait]
impl Source for Asura {
    fn meta(&self) -> &SourceMeta {
        &self.meta
    }

    fn parse_url(&self, url: &Url) -> Option<UrlTarget> {
        let host = url.host_str()?;
        if !is_asura_host(host) && Some(host) != self.meta.base_url.host_str() {
            return None;
        }
        let segments: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
        match segments.as_slice() {
            // asuracomic.net used /series/, asurascans.com uses /comics/.
            ["comics" | "series", slug] => Some(UrlTarget::Series(parse::bare_slug(slug).into())),
            ["comics" | "series", slug, "chapter", number] => Some(UrlTarget::Chapter(
                format!("{}/{number}", parse::bare_slug(slug)).into(),
            )),
            _ => None,
        }
    }

    async fn search(&self, query: &str, _: &str) -> Result<Vec<SeriesHit>> {
        let mut url = self.url("/browse");
        url.query_pairs_mut().append_pair("search", query);
        let html = self.site.get_text(&url).await?;
        Ok(parse::cards(&html, &self.meta.base_url))
    }

    async fn series(&self, key: &SeriesKey, _: &str) -> Result<Series> {
        let url = self.url(&format!("/comics/{key}"));
        let html = self.site.get_text(&url).await?;
        parse::series(&html, key, &url)
    }

    async fn chapters(&self, key: &SeriesKey, _: &str) -> Result<Vec<ChapterInfo>> {
        let html = self
            .site
            .get_text(&self.url(&format!("/comics/{key}")))
            .await?;
        parse::chapters(&html, key.as_str(), &self.meta.base_url, Utc::now())
    }

    async fn pages(&self, chapter: &ChapterKey) -> Result<Vec<Url>> {
        // Keys are "{slug}/{number}".
        let (slug, number) = chapter.as_str().rsplit_once('/').ok_or_else(|| {
            kaze_core::SourceError::parse("chapter key", format!("unexpected {chapter}"))
        })?;
        let html = self
            .site
            .get_text(&self.url(&format!("/comics/{slug}/chapter/{number}")))
            .await?;
        parse::pages(&html)
    }

    async fn fetch_image(&self, url: &Url) -> Result<Image> {
        self.images.get_image(url).await
    }

    async fn latest_updates(&self) -> Result<Vec<SeriesKey>> {
        // The browse page lists series by latest update unless told otherwise.
        let html = self.site.get_text(&self.url("/browse")).await?;
        Ok(parse::cards(&html, &self.meta.base_url)
            .into_iter()
            .map(|hit| hit.key)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    use super::*;
    use crate::http::HttpConfig;

    fn source(base: &str) -> Asura {
        Asura::with_base_url(
            &Http::new(HttpConfig::default()).unwrap(),
            base.parse().unwrap(),
        )
    }

    #[test]
    fn recognises_current_and_old_urls() {
        let asura = source(BASE_URL);
        let parse = |u: &str| asura.parse_url(&u.parse().unwrap());
        assert_eq!(
            parse("https://asurascans.com/comics/solo-leveling-bd5bdaf8"),
            Some(UrlTarget::Series("solo-leveling".into()))
        );
        assert_eq!(
            parse("https://asuracomic.net/series/solo-leveling-0a1b2c3d"),
            Some(UrlTarget::Series("solo-leveling".into()))
        );
        assert_eq!(
            parse("https://asurascans.com/comics/solo-leveling-bd5bdaf8/chapter/12"),
            Some(UrlTarget::Chapter("solo-leveling/12".into()))
        );
        assert_eq!(parse("https://asurascans.com/browse?search=x"), None);
        assert_eq!(parse("https://mangadex.org/comics/solo-leveling"), None);
    }

    #[tokio::test]
    async fn search_reads_series_cards() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/browse"))
            .and(query_param("search", "solo"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(include_str!("fixtures/browse.html")),
            )
            .expect(1)
            .mount(&server)
            .await;
        let hits = source(&server.uri()).search("solo", "en").await.unwrap();
        assert_eq!(hits.len(), 4);
        let hit = &hits[0];
        assert_eq!(hit.title, "Only I Have an EX-Grade Summon");
        assert_eq!(hit.key.as_str(), "only-i-have-an-ex-grade-summon");
        assert!(
            hit.url
                .as_str()
                .ends_with("/comics/only-i-have-an-ex-grade-summon")
        );
        assert!(
            hit.cover_url
                .as_ref()
                .unwrap()
                .as_str()
                .contains("covers/only-i-have-an-ex-grade-summon")
        );
    }

    #[test]
    fn series_metadata_comes_from_the_description_island() {
        let url: Url = "https://asurascans.com/comics/solo-max-level-newbie"
            .parse()
            .unwrap();
        let series = parse::series(
            include_str!("fixtures/series.html"),
            &"solo-max-level-newbie".into(),
            &url,
        )
        .unwrap();
        assert_eq!(series.title, "Solo Max-Level Newbie");
        assert_eq!(series.authors, ["Maslow", "Swingbat"]);
        assert_eq!(series.status, kaze_core::SeriesStatus::Ongoing);
        assert!(
            series
                .alt_titles
                .iter()
                .any(|t| t == "Na Honja Mallep Nyubi")
        );
        assert!(series.tags.iter().any(|t| t == "Action"));
        let description = series.description.unwrap();
        assert!(description.starts_with("Jinhyuk"), "{description}");
        assert!(!description.contains("<p>"));
    }

    #[test]
    fn chapters_skip_premium_and_early_access() {
        let base: Url = BASE_URL.parse().unwrap();
        let now = Utc.with_ymd_and_hms(2026, 10, 10, 0, 0, 0).unwrap();
        let chapters = parse::chapters(
            include_str!("fixtures/series.html"),
            "solo-max-level-newbie",
            &base,
            now,
        )
        .unwrap();
        let numbers: Vec<String> = chapters.iter().map(|c| c.number.to_string()).collect();
        // The fixture's newest is premium and the next is in early access.
        assert_eq!(numbers, ["277", "276", "275"]);
        assert_eq!(chapters[0].key.as_str(), "solo-max-level-newbie/277");
        assert_eq!(
            chapters[0].url.as_str(),
            "https://asurascans.com/comics/solo-max-level-newbie/chapter/277"
        );
        assert_eq!(
            chapters[0].title.as_deref(),
            Some("A New Ancient Species (1)")
        );
    }

    #[tokio::test]
    async fn pages_come_from_the_reader_island() {
        let server = MockServer::start().await;
        Mock::given(path("/comics/solo-max-level-newbie/chapter/278"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(include_str!("fixtures/chapter.html")),
            )
            .expect(1)
            .mount(&server)
            .await;
        let pages = source(&server.uri())
            .pages(&"solo-max-level-newbie/278".into())
            .await
            .unwrap();
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].host_str(), Some("cdn.asurascans.com"));
    }

    /// Hits the real site: `just test-live`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_solo_leveling() {
        let asura = Asura::new(&Http::new(HttpConfig::default()).unwrap());
        let hits = asura.search("solo leveling", "en").await.unwrap();
        let hit = hits.iter().find(|h| h.title == "Solo Leveling").unwrap();
        let series = asura.series(&hit.key, "en").await.unwrap();
        assert_eq!(series.title, "Solo Leveling");
        let chapters = asura.chapters(&hit.key, "en").await.unwrap();
        assert!(chapters.len() > 100);
        let pages = asura.pages(&chapters[0].key).await.unwrap();
        let image = asura.fetch_image(&pages[0]).await.unwrap();
        assert!(image.bytes.len() > 1000);
        assert_ne!(asura.latest_updates().await.unwrap().len(), 0);
    }
}
