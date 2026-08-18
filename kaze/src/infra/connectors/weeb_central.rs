use async_trait::async_trait;
use scraper::{Html, Selector};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use scraper::Element;

use crate::domain::error::DomainError;
use crate::domain::manga_source::{ChapterCandidate, MangaCandidate, MangaSource};
use crate::http::{HttpClient, RequestType};
use crate::infra::connectors::chromium::BrowserManager;
use crate::utils::normalize_chapter_number;

pub struct WeebCentralSource {
    http: Arc<dyn HttpClient>,
    browser: Option<Arc<BrowserManager>>,
}

impl WeebCentralSource {
    pub fn new(http: Arc<dyn HttpClient>, browser: Option<Arc<BrowserManager>>) -> Self {
        Self { http, browser }
    }
}

const BASE_URL: &str = "https://weebcentral.com";

#[async_trait]
impl MangaSource for WeebCentralSource {
    fn name(&self) -> &str {
        "WeebCentral"
    }

    fn matches_url(&self, url: &str) -> bool {
        url.contains("weebcentral.com")
    }

    async fn search(&self, query: &str) -> Result<Vec<MangaCandidate>, DomainError> {
        // Use POST to `/search/simple?location=main` -- this is what the HTMX frontend does.
        let encoded = urlencoding::encode(query);
        let url = format!("{BASE_URL}/search/simple?location=main");
        let body = format!("text={encoded}&display_mode=STRING");
        let resp = self
            .http
            .post_request(
                &url,
                &body,
                "application/x-www-form-urlencoded",
                CancellationToken::new(),
            )
            .await
            .map_err(|e| DomainError::source(e.to_string()))?;

        let doc = Html::parse_document(&resp.body);
        let sel = Selector::parse("a.join-item")
            .map_err(|e| DomainError::invalid(format!("selector: {e}")))?;

        let mut results = Vec::new();
        for el in doc.select(&sel) {
            let href = el.value().attr("href").unwrap_or("").to_string();
            let name = Selector::parse(".line-clamp-2")
                .ok()
                .and_then(|s| el.select(&s).next())
                .and_then(|e| e.text().next())
                .map(|t| t.trim().to_string())
                .unwrap_or_default();

            if name.is_empty() || href.is_empty() {
                continue;
            }

            let site_id = extract_site_id(&href);
            let cover_url = Selector::parse("img")
                .ok()
                .and_then(|s| el.select(&s).next())
                .and_then(|e| e.value().attr("src"))
                .map(|s| make_abs(s, "https://temp.compsci88.com"))
                .unwrap_or_default();

            results.push(MangaCandidate {
                name,
                description: String::new(),
                cover_url,
                source_id: site_id,
                website_url: Some(href),
                release_status: Some("Continuing".into()),
                authors: vec![],
                tags: vec![],
                year: None,
                original_language: Some("en".into()),
            });
        }
        Ok(results)
    }

    async fn from_url(&self, url: &str) -> Result<Option<MangaCandidate>, DomainError> {
        let site_id = match regex_lazy().captures(url) {
            Some(c) => c.get(1).unwrap().as_str().to_string(),
            None => return Ok(None),
        };

        let page_url = format!("{BASE_URL}/series/{site_id}");
        let resp = self
            .http
            .make_request(
                &page_url,
                RequestType::Default,
                None,
                CancellationToken::new(),
            )
            .await
            .map_err(|e| DomainError::source(e.to_string()))?;

        if !(200..300).contains(&resp.status) {
            return Ok(None);
        }

        parse_manga_page(&resp.body, &site_id, &page_url).map(Some)
    }

    async fn chapters(&self, source_id: &str) -> Result<Vec<ChapterCandidate>, DomainError> {
        let url = format!("{BASE_URL}/series/{source_id}/full-chapter-list");
        let resp = self
            .http
            .make_request(&url, RequestType::Default, None, CancellationToken::new())
            .await
            .map_err(|e| DomainError::source(e.to_string()))?;

        if !(200..300).contains(&resp.status) {
            return Ok(Vec::new());
        }

        Ok(parse_chapter_list(&resp.body))
    }

    async fn page_images(&self, chapter_source_id: &str) -> Result<Vec<String>, DomainError> {
        let url = format!("{BASE_URL}/{chapter_source_id}");
        let browser = self
            .browser
            .as_ref()
            .ok_or_else(|| DomainError::invalid("Chromium required for WeebCentral images"))?;

        let html = browser
            .fetch_html(&url)
            .await
            .map_err(|e| DomainError::source(e.to_string()))?;

        Ok(extract_page_images(&html))
    }

    fn image_referrer(&self) -> Option<&str> {
        Some(BASE_URL)
    }
}

// ── Helpers ──

fn regex_lazy() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"https?://(?:www\.)?weebcentral\.com/(?:series|chapters)/([^/]+)")
            .unwrap()
    })
}

fn extract_site_id(href: &str) -> String {
    regex_lazy()
        .captures(href)
        .map(|c| c.get(1).unwrap().as_str().to_string())
        .unwrap_or_else(|| {
            href.trim_start_matches('/')
                .trim_start_matches("https://weebcentral.com/")
                .split('/')
                .next()
                .unwrap_or("")
                .to_string()
        })
}

fn make_abs(url: &str, base: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if url.starts_with("//") {
        format!("https:{}", url)
    } else if url.starts_with('/') {
        format!("{}{}", base, url)
    } else {
        url.to_string()
    }
}

fn parse_chapter_list(html: &str) -> Vec<ChapterCandidate> {
    let doc = Html::parse_document(html);
    let sel = match Selector::parse("a[href*='/chapters/']") {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let mut chapters = Vec::new();
    for el in doc.select(&sel) {
        let href = el.value().attr("href").unwrap_or("").to_string();
        if !href.contains("/chapters/") {
            continue;
        }

        let ch_text = el
            .text()
            .find(|t| t.trim().starts_with("Chapter "))
            .unwrap_or_default()
            .trim()
            .to_string();

        let ch_num = normalize_chapter_number(&ch_text.replace("Chapter ", "").trim());
        if ch_num.is_empty() {
            continue;
        }

        let source_id = href
            .strip_prefix(BASE_URL)
            .unwrap_or(&href)
            .trim_start_matches('/')
            .to_string();

        chapters.push(ChapterCandidate {
            chapter_number: ch_num,
            volume_number: None,
            title: if ch_text.is_empty() {
                None
            } else {
                Some(ch_text)
            },
            source_id,
            website_url: Some(href),
        });
    }

    chapters.reverse();
    chapters
}

fn extract_page_images(html: &str) -> Vec<String> {
    let doc = Html::parse_document(html);
    let sel = match Selector::parse("img") {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let valid_exts = [".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif"];
    let mut urls = Vec::new();

    for img in doc.select(&sel) {
        let alt = img.value().attr("alt").unwrap_or("");
        if !alt.starts_with("Page") {
            continue;
        }

        let src = img
            .value()
            .attr("data-src")
            .or_else(|| img.value().attr("src"))
            .unwrap_or("");

        if src.is_empty() {
            continue;
        }

        let abs = make_abs(src, BASE_URL);
        let lower = abs.to_lowercase();
        if valid_exts.iter().any(|ext| lower.contains(ext)) || abs.contains('?') {
            urls.push(abs);
        }
    }

    urls
}

fn parse_manga_page(html: &str, site_id: &str, url: &str) -> Result<MangaCandidate, DomainError> {
    let doc = Html::parse_document(html);

    let title = Selector::parse("title")
        .ok()
        .and_then(|s| doc.select(&s).next())
        .and_then(|e| e.text().next())
        .map(|t| t.split('|').next().unwrap_or(t).trim().to_string())
        .unwrap_or_else(|| site_id.to_string());

    let cover_url = Selector::parse("img[alt*='cover' i], img[alt*='Cover']")
        .ok()
        .and_then(|s| doc.select(&s).next())
        .and_then(|e| e.value().attr("src"))
        .map(|s| make_abs(s, "https://temp.compsci88.com"))
        .unwrap_or_default();

    let description = extract_sibling_text(&doc, "Description");
    let release_status = extract_status(&doc);
    let authors = extract_sibling_links(&doc, "Author");
    let tags = extract_sibling_links(&doc, "Tag");

    let year = Selector::parse("strong")
        .ok()
        .and_then(|s| {
            doc.select(&s)
                .find(|e| e.text().collect::<String>().starts_with("Released"))
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|e| e.text().next())
        .and_then(|t| t.split_whitespace().last()?.parse::<u32>().ok());

    Ok(MangaCandidate {
        name: title,
        description,
        cover_url,
        source_id: site_id.to_string(),
        website_url: Some(url.to_string()),
        release_status: Some(release_status),
        authors,
        tags,
        year,
        original_language: Some("en".into()),
    })
}

fn extract_sibling_text(doc: &Html, label: &str) -> String {
    Selector::parse("strong")
        .ok()
        .and_then(|s| {
            doc.select(&s)
                .find(|e| e.text().collect::<String>().starts_with(label))
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|e| e.text().next().map(|t| t.trim().to_string()))
        .unwrap_or_default()
}

fn extract_sibling_links(doc: &Html, label: &str) -> Vec<String> {
    Selector::parse("strong")
        .ok()
        .and_then(|s| {
            doc.select(&s)
                .find(|e| e.text().collect::<String>().starts_with(label))
        })
        .and_then(|e| e.parent_element())
        .and_then(|parent| {
            let a_sel = Selector::parse("a").ok()?;
            Some(
                parent
                    .select(&a_sel)
                    .map(|e| e.text().collect::<String>())
                    .collect::<Vec<_>>(),
            )
        })
        .map(|v| v.into_iter().filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

fn extract_status(doc: &Html) -> String {
    Selector::parse("strong")
        .ok()
        .and_then(|s| {
            doc.select(&s)
                .find(|e| e.text().collect::<String>().starts_with("Status"))
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|e| e.text().next().map(|t| t.trim().to_lowercase()))
        .map(|t| match t.as_str() {
            "ongoing" => "Continuing",
            "hiatus" => "OnHiatus",
            "completed" | "complete" => "Completed",
            "canceled" | "cancelled" => "Cancelled",
            _ => "Continuing",
        })
        .unwrap_or("Continuing")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::http::mock::MockHttpClient;

    #[tokio::test]
    async fn search_parses_results() {
        let mock = MockHttpClient::builder()
            .expect_get(
                "weebcentral.com/search/simple",
                include_str!("../../connectors/fixtures/weebcentral_search.html"),
            )
            .build();
        let source = WeebCentralSource::new(Arc::new(mock), None);
        let results = source.search("solo").await.unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "Solo Leveling");
        assert_eq!(results[0].source_id, "ABC");
    }

    #[test]
    fn url_matches_weebcentral() {
        let mock = MockHttpClient::builder().build();
        let source = WeebCentralSource::new(Arc::new(mock), None);
        assert!(source.matches_url("https://weebcentral.com/series/abc123"));
        assert!(!source.matches_url("https://example.com"));
    }

    #[tokio::test]
    async fn get_chapters_fails_without_browser() {
        let mock = MockHttpClient::builder().build();
        let source = WeebCentralSource::new(Arc::new(mock), None);
        let result = source.chapters("solo-leveling").await;
        assert!(result.is_err());
    }

    #[test]
    fn extract_page_images_filters_non_page_alts() {
        let html = include_str!("../../connectors/fixtures/weebcentral_chapter.html");
        let urls = extract_page_images(html);
        assert_eq!(urls.len(), 3, "expected 3 page images, got: {urls:?}");
        assert!(urls[0].contains("page1.jpg"));
    }

    #[test]
    fn parse_chapter_list_extracts_chapters() {
        let html = include_str!("../../connectors/fixtures/weebcentral_chapter_list.html");
        let chapters = parse_chapter_list(html);
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].chapter_number, "1");
        assert_eq!(chapters[2].chapter_number, "3");
    }

    #[test]
    fn parse_manga_page_extracts_fields() {
        let html = include_str!("../../connectors/fixtures/weebcentral_manga.html");
        let result = parse_manga_page(
            html,
            "solo-leveling",
            "https://weebcentral.com/series/solo-leveling",
        )
        .unwrap();
        assert_eq!(result.name, "Solo Leveling");
        assert_eq!(result.tags, vec!["Action", "Fantasy"]);
        assert_eq!(result.year, Some(2018));
    }
}
