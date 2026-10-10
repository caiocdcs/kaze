//! [MangaDex](https://mangadex.org) through its official JSON API. Pages
//! come from MangaDex@Home servers, which the API hands out per chapter.

mod model;

use async_trait::async_trait;
use kaze_core::{
    ChapterInfo, ChapterKey, Image, RateLimit, Result, Series, SeriesHit, SeriesKey, Source,
    SourceMeta, UrlTarget,
};
use url::Url;

use self::model::{AtHome, Chapter, Collection, Entity, Manga};
use crate::http::{Http, SourceHttp};

const SITE_URL: &str = "https://mangadex.org";
const API_URL: &str = "https://api.mangadex.org";
/// The API asks clients to say who they are rather than pose as a browser.
const USER_AGENT: &str = concat!("kaze/", env!("CARGO_PKG_VERSION"));
/// Chapters in this language are downloaded.
const LANGUAGE: &str = "en";
/// Everything but pornographic, which MangaDex also hides by default.
const RATINGS: [&str; 3] = ["safe", "suggestive", "erotica"];
/// The feed endpoint's maximum page size.
const FEED_PAGE: usize = 500;

pub struct MangaDex {
    meta: SourceMeta,
    api_url: Url,
    api: SourceHttp,
    images: SourceHttp,
}

impl MangaDex {
    #[must_use]
    pub fn new(http: &Http) -> Self {
        Self::with_api_url(http, Url::parse(API_URL).expect("valid api url"))
    }

    /// Point the API at another host; tests use this with a mock server.
    #[must_use]
    pub fn with_api_url(http: &Http, api_url: Url) -> Self {
        // The documented global limit is 5 requests per second.
        let rate_limit = RateLimit::per_second(4);
        Self {
            api: http
                .for_source(rate_limit, None)
                .with_user_agent(USER_AGENT),
            images: http
                .for_source(RateLimit::per_second(8), None)
                .with_user_agent(USER_AGENT),
            api_url,
            meta: SourceMeta {
                id: "mangadex",
                name: "MangaDex",
                base_url: Url::parse(SITE_URL).expect("valid site url"),
                languages: &[LANGUAGE],
                rate_limit,
            },
        }
    }

    fn url(&self, path: &str, query: &[(&str, &str)]) -> Url {
        let mut url = self.api_url.join(path).expect("path joins onto api url");
        {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
            for rating in RATINGS {
                pairs.append_pair("contentRating[]", rating);
            }
        }
        url
    }
}

/// MangaDex ids are UUIDs.
fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            matches!(i, 8 | 13 | 18 | 23) == (b == b'-') && (b == b'-' || b.is_ascii_hexdigit())
        })
}

#[async_trait]
impl Source for MangaDex {
    fn meta(&self) -> &SourceMeta {
        &self.meta
    }

    fn parse_url(&self, url: &Url) -> Option<UrlTarget> {
        if url.host_str()?.trim_start_matches("www.") != "mangadex.org" {
            return None;
        }
        let mut segments = url.path_segments()?;
        match (segments.next()?, segments.next()?) {
            ("title", id) if is_uuid(id) => Some(UrlTarget::Series(id.into())),
            ("chapter", id) if is_uuid(id) => Some(UrlTarget::Chapter(id.into())),
            _ => None,
        }
    }

    async fn search(&self, query: &str) -> Result<Vec<SeriesHit>> {
        let url = self.url(
            "/manga",
            &[
                ("title", query),
                ("limit", "20"),
                ("includes[]", "cover_art"),
                ("order[relevance]", "desc"),
                // Hide series with nothing to download in our language.
                ("hasAvailableChapters", "true"),
                ("availableTranslatedLanguage[]", LANGUAGE),
            ],
        );
        let found: Collection<Manga> = self.api.get_json(&url).await?;
        Ok(found.data.iter().map(Manga::hit).collect())
    }

    async fn series(&self, key: &SeriesKey) -> Result<Series> {
        let url = self.url(
            &format!("/manga/{key}"),
            &[
                ("includes[]", "cover_art"),
                ("includes[]", "author"),
                ("includes[]", "artist"),
            ],
        );
        let manga: Entity<Manga> = self.api.get_json(&url).await?;
        Ok(manga.data.series(LANGUAGE))
    }

    async fn chapters(&self, key: &SeriesKey) -> Result<Vec<ChapterInfo>> {
        let limit = FEED_PAGE.to_string();
        let mut chapters = Vec::new();
        let mut offset = 0;
        loop {
            let offset_text = offset.to_string();
            let url = self.url(
                &format!("/manga/{key}/feed"),
                &[
                    ("translatedLanguage[]", LANGUAGE),
                    ("limit", &limit),
                    ("offset", &offset_text),
                    ("order[chapter]", "asc"),
                    ("includes[]", "scanlation_group"),
                ],
            );
            let page: Collection<Chapter> = self.api.get_json(&url).await?;
            let fetched = page.data.len();
            chapters.extend(
                page.data
                    .iter()
                    .filter_map(|c| c.info().map(|info| (info, c.attributes.pages))),
            );
            offset += fetched;
            if fetched == 0 || offset >= page.total {
                break;
            }
        }
        Ok(model::one_per_number(chapters))
    }

    async fn pages(&self, chapter: &ChapterKey) -> Result<Vec<Url>> {
        let url = self
            .api_url
            .join(&format!("/at-home/server/{chapter}"))
            .expect("path joins onto api url");
        let home: AtHome = self.api.get_json(&url).await?;
        home.chapter
            .data
            .iter()
            .map(|file| {
                home.base_url
                    .join(&format!("data/{}/{file}", home.chapter.hash))
                    .map_err(|e| kaze_core::SourceError::parse("page URL", e.to_string()))
            })
            .collect()
    }

    async fn fetch_image(&self, url: &Url) -> Result<Image> {
        self.images.get_image(url).await
    }

    async fn latest_updates(&self) -> Result<Vec<SeriesKey>> {
        let url = self.url(
            "/chapter",
            &[
                ("translatedLanguage[]", LANGUAGE),
                ("order[readableAt]", "desc"),
                ("limit", "100"),
                ("includeExternalUrl", "0"),
            ],
        );
        let latest: Collection<Chapter> = self.api.get_json(&url).await?;
        let mut keys: Vec<SeriesKey> = Vec::new();
        for chapter in &latest.data {
            for r in chapter.relationships.iter().filter(|r| r.kind == "manga") {
                let key = SeriesKey::from(r.id.as_str());
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path, query_param},
    };

    use super::*;
    use crate::http::HttpConfig;

    const APOTHECARY: &str = "e18fe8c6-f6dc-4f05-8462-7b2083ff9a6c";

    fn source(api: &str) -> MangaDex {
        MangaDex::with_api_url(
            &Http::new(HttpConfig::default()).unwrap(),
            api.parse().unwrap(),
        )
    }

    async fn serve(route: &str, fixture: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(route))
            .and(header("user-agent", USER_AGENT))
            .respond_with(ResponseTemplate::new(200).set_body_string(fixture))
            .expect(1)
            .mount(&server)
            .await;
        server
    }

    #[test]
    fn recognises_urls() {
        let md = source(API_URL);
        let parse = |u: &str| md.parse_url(&u.parse().unwrap());
        assert_eq!(
            parse(&format!("https://mangadex.org/title/{APOTHECARY}/kusuriya")),
            Some(UrlTarget::Series(APOTHECARY.into()))
        );
        assert_eq!(
            parse("https://mangadex.org/chapter/da5d1841-dea9-48a9-91f0-ddaf63b75040"),
            Some(UrlTarget::Chapter(
                "da5d1841-dea9-48a9-91f0-ddaf63b75040".into()
            ))
        );
        assert_eq!(parse("https://mangadex.org/title/not-a-uuid"), None);
        assert_eq!(
            parse(&format!("https://weebcentral.com/title/{APOTHECARY}")),
            None
        );
    }

    #[tokio::test]
    async fn search_maps_hits_with_covers() {
        let server = serve("/manga", include_str!("fixtures/search.json")).await;
        let hits = source(&server.uri())
            .search("apothecary diaries")
            .await
            .unwrap();
        let hit = hits.iter().find(|h| h.key.as_str() == APOTHECARY).unwrap();
        assert_eq!(hit.title, "The Apothecary Diaries");
        assert!(
            hit.cover_url
                .as_ref()
                .unwrap()
                .as_str()
                .starts_with(&format!(
                    "https://uploads.mangadex.org/covers/{APOTHECARY}/"
                ))
        );
    }

    #[tokio::test]
    async fn series_prefers_english_and_lists_people() {
        let server = serve(
            &format!("/manga/{APOTHECARY}"),
            include_str!("fixtures/manga.json"),
        )
        .await;
        let series = source(&server.uri())
            .series(&APOTHECARY.into())
            .await
            .unwrap();
        assert_eq!(series.title, "The Apothecary Diaries");
        assert!(
            series
                .alt_titles
                .iter()
                .any(|t| t == "Kusuriya no Hitorigoto")
        );
        assert_eq!(
            series.authors,
            ["Hyuuga Natsu", "Nanao Itsuki", "Nekokurage"]
        );
        assert_eq!(series.year, Some(2017));
        assert!(series.description.is_some());
        assert_ne!(series.tags.len(), 0);
    }

    #[tokio::test]
    async fn chapters_skip_external_links() {
        let server = MockServer::start().await;
        Mock::given(path(format!("/manga/{APOTHECARY}/feed")))
            .and(query_param("translatedLanguage[]", "en"))
            .and(query_param("offset", "0"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(include_str!("fixtures/feed.json")),
            )
            .expect(1)
            .mount(&server)
            .await;
        let chapters = source(&server.uri())
            .chapters(&APOTHECARY.into())
            .await
            .unwrap();
        let numbers: Vec<String> = chapters.iter().map(|c| c.number.to_string()).collect();
        assert_eq!(
            numbers,
            ["1", "2", "3", "4"],
            "the three publisher links are left out"
        );
        assert_eq!(chapters[0].scanlator.as_deref(), Some("Knox"));
        assert_eq!(chapters[0].volume, Some(1));
    }

    #[tokio::test]
    async fn pages_come_from_the_at_home_server() {
        let server = serve(
            "/at-home/server/97193d1f-ce85-4f2c-8d8a-d65ece82a0b4",
            include_str!("fixtures/at-home.json"),
        )
        .await;
        let pages = source(&server.uri())
            .pages(&"97193d1f-ce85-4f2c-8d8a-d65ece82a0b4".into())
            .await
            .unwrap();
        assert_eq!(pages.len(), 35);
        assert!(pages[0].path().starts_with("/data/"));
    }

    #[tokio::test]
    async fn latest_updates_lists_each_series_once() {
        let server = serve("/chapter", include_str!("fixtures/latest.json")).await;
        let keys = source(&server.uri()).latest_updates().await.unwrap();
        assert_ne!(keys.len(), 0);
        let mut unique = keys.clone();
        unique.dedup();
        assert_eq!(keys.len(), unique.len());
    }

    #[test]
    fn duplicate_uploads_keep_the_fullest() {
        let info = |key: &str| ChapterInfo {
            key: key.into(),
            number: "5".parse().unwrap(),
            volume: None,
            title: None,
            url: "https://mangadex.org/chapter/x".parse().unwrap(),
            language: None,
            scanlator: None,
            published_at: None,
        };
        let kept = model::one_per_number(vec![(info("a"), 20), (info("b"), 24), (info("c"), 24)]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].key.as_str(), "b");
    }

    /// Hits the real API: `just test-live`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_apothecary_diaries() {
        let md = MangaDex::new(&Http::new(HttpConfig::default()).unwrap());
        let hits = md.search("apothecary diaries").await.unwrap();
        assert_ne!(hits.len(), 0);
        let series = md.series(&APOTHECARY.into()).await.unwrap();
        assert_eq!(series.title, "The Apothecary Diaries");
        let latest = md.latest_updates().await.unwrap();
        let key = latest.first().unwrap();
        let chapters = md.chapters(key).await.unwrap();
        let pages = md.pages(&chapters.last().unwrap().key).await.unwrap();
        let image = md.fetch_image(&pages[0]).await.unwrap();
        assert!(image.bytes.len() > 1000);
    }
}
