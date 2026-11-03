//! [AniList](https://anilist.co) as a metadata provider: title, summary,
//! genres, staff and cover of a manga, apart from the sites chapters come
//! from. Its GraphQL API needs no key.

mod model;

use kaze_core::{AniListId, ExternalIds, RateLimit, Result, Series, SeriesKey};
use serde_json::json;
use url::Url;

pub use self::model::{Format, Media, Role, Staff, Titles};
use self::model::{MediaData, PageData, Request, Response};
use crate::{
    http::{Http, SourceHttp},
    literal::url,
};

const API_URL: &str = "https://graphql.anilist.co";
const SITE_URL: &str = "https://anilist.co";
const USER_AGENT: &str = concat!("kaze/", env!("CARGO_PKG_VERSION"));

pub struct AniList {
    api_url: Url,
    api: SourceHttp,
}

impl AniList {
    #[must_use]
    pub fn new(http: &Http) -> Self {
        Self::with_api_url(http, url!(API_URL))
    }

    /// Point at another host; tests use this with a mock server.
    #[must_use]
    pub fn with_api_url(http: &Http, api_url: Url) -> Self {
        Self {
            // The documented limit is 90 a minute, lowered to 30 at times.
            // kaze needs about one request per series a week.
            api: http
                .for_source(RateLimit::per_minute(20), None)
                .with_user_agent(USER_AGENT),
            api_url,
        }
    }

    /// Manga (not novels) matching a title, best first by AniList's ranking.
    pub async fn search(&self, title: &str) -> Result<Vec<Media>> {
        let query = model::search_query();
        let request = Request {
            query: &query,
            variables: json!({ "search": plain_quotes(title) }),
        };
        let response: Response<PageData> = self.api.post_json(&self.api_url, &request).await?;
        Ok(response
            .data
            .page
            .media
            .into_iter()
            .map(Media::from)
            .collect())
    }

    /// One manga by id. An unknown id is [`kaze_core::SourceError::NotFound`].
    pub async fn media(&self, id: AniListId) -> Result<Media> {
        let query = model::media_query();
        let request = Request {
            query: &query,
            variables: json!({ "id": id.0 }),
        };
        let response: Response<MediaData> = self.api.post_json(&self.api_url, &request).await?;
        Ok(response.data.media.into())
    }
}

/// In order, without repeats.
fn unique(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// AniList's search finds nothing for "The Novel’s Extra" but finds
/// "The Novel's Extra".
fn plain_quotes(title: &str) -> String {
    title
        .chars()
        .map(|c| match c {
            '‘' | '’' | '‛' | '′' => '\'',
            '“' | '”' | '″' => '"',
            c => c,
        })
        .collect()
}

impl Media {
    /// The English title when AniList has one, else the romanized one.
    #[must_use]
    pub fn title(&self) -> String {
        let t = &self.titles;
        t.english
            .clone()
            .or_else(|| t.romaji.clone())
            .or_else(|| t.native.clone())
            .unwrap_or_else(|| self.id.to_string())
    }

    #[must_use]
    pub fn url(&self) -> Url {
        let mut url = url!(SITE_URL);
        url.set_path(&format!("/manga/{}", self.id));
        url
    }

    /// As a [`Series`], the shape the library stores metadata in. Story
    /// writers come before artists; genres before tags.
    #[must_use]
    pub fn into_series(self) -> Series {
        let title = self.title();
        let url = self.url();
        let names = [self.titles.english, self.titles.romaji, self.titles.native];
        let alt_titles = unique(
            names
                .into_iter()
                .flatten()
                .chain(self.synonyms)
                .filter(|t| *t != title),
        );
        let by_role = |role| self.staff.iter().filter(move |s: &&Staff| s.role == role);
        let authors = unique(
            by_role(Role::StoryAndArt)
                .chain(by_role(Role::Story))
                .chain(by_role(Role::Art))
                .map(|s| s.name.clone()),
        );
        let tags = unique(self.genres.into_iter().chain(self.tags));
        Series {
            key: SeriesKey(self.id.to_string()),
            title,
            alt_titles,
            url,
            cover_url: self.cover_url,
            description: self.description,
            authors,
            tags,
            status: self.status,
            year: self.year,
            language: None,
            ids: ExternalIds {
                anilist: Some(self.id),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use kaze_core::{SeriesStatus, SourceError};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, header, method},
    };

    use super::*;
    use crate::http::HttpConfig;

    const FRIEREN: AniListId = AniListId(118_586);

    fn client(api: &str) -> AniList {
        AniList::with_api_url(
            &Http::new(HttpConfig::default()).unwrap(),
            api.parse().unwrap(),
        )
    }

    async fn serve(variables: serde_json::Value, status: u16, fixture: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("user-agent", USER_AGENT))
            .and(header("content-type", "application/json"))
            .and(body_partial_json(json!({ "variables": variables })))
            .respond_with(ResponseTemplate::new(status).set_body_string(fixture))
            .expect(1)
            .mount(&server)
            .await;
        server
    }

    #[test]
    fn makes_quotes_plain() {
        assert_eq!(plain_quotes("The Novel’s “Extra”"), "The Novel's \"Extra\"");
    }

    #[tokio::test]
    async fn searches() {
        let server = serve(
            json!({ "search": "frieren" }),
            200,
            include_str!("fixtures/search.json"),
        )
        .await;
        let hits = client(&server.uri()).search("frieren").await.unwrap();
        assert_ne!(hits.len(), 0);
        let first = &hits[0];
        assert_eq!(first.id, FRIEREN);
        assert_eq!(first.titles.romaji.as_deref(), Some("Sousou no Frieren"));
        assert_eq!(first.format, Some(Format::Manga));
        assert!(first.popularity > 0);
        assert!(
            hits.iter().all(|m| m.format != Some(Format::Novel)),
            "novels are filtered out by the query"
        );
    }

    #[tokio::test]
    async fn reads_one_media() {
        let server = serve(
            json!({ "id": FRIEREN.0 }),
            200,
            include_str!("fixtures/media.json"),
        )
        .await;
        let media = client(&server.uri()).media(FRIEREN).await.unwrap();
        assert_eq!(media.status, SeriesStatus::Ongoing);
        assert_eq!(media.year, Some(2020));
        assert!(media.genres.contains(&"Fantasy".to_owned()));
        let description = media.description.clone().unwrap();
        assert!(
            !description.contains('<'),
            "markup is stripped: {description}"
        );
        assert!(media.cover_url.is_some());
        assert!(media.staff.iter().any(|s| s.role == Role::Story));
        assert!(media.staff.iter().any(|s| s.role == Role::Art));

        let series = media.into_series();
        assert_eq!(series.key.as_str(), "118586");
        assert_eq!(series.title, "Frieren: Beyond Journey’s End");
        assert!(series.alt_titles.contains(&"Sousou no Frieren".to_owned()));
        assert_eq!(series.url.as_str(), "https://anilist.co/manga/118586");
        assert_eq!(
            series.authors.len(),
            2,
            "writer then artist: {:?}",
            series.authors
        );
        assert_eq!(series.tags.first().map(String::as_str), Some("Adventure"));
    }

    #[tokio::test]
    async fn unknown_id_is_not_found() {
        let server = serve(
            json!({ "id": 1 }),
            404,
            r#"{"data":{"Media":null},"errors":[{"message":"Not Found.","status":404}]}"#,
        )
        .await;
        let err = client(&server.uri()).media(AniListId(1)).await.unwrap_err();
        assert!(matches!(err, SourceError::NotFound { .. }), "{err}");
    }

    /// Hits the real API: `just test-live`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_frieren() {
        let anilist = AniList::new(&Http::new(HttpConfig::default()).unwrap());
        let hits = anilist.search("Sousou no Frieren").await.unwrap();
        assert_eq!(hits.first().map(|m| m.id), Some(FRIEREN));
        let media = anilist.media(FRIEREN).await.unwrap();
        assert_eq!(media.titles.romaji.as_deref(), Some("Sousou no Frieren"));
    }
}
