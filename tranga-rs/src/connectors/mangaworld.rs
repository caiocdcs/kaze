use async_trait::async_trait;
use scraper::Element;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::http::{HttpClient, RequestType};
use super::{BrowserManager, MangaConnector, NewChapterResult, NewMangaResult};

pub struct MangaworldConnector {
    enabled: bool,
    browser: Option<Arc<BrowserManager>>,
}
impl Default for MangaworldConnector {
    fn default() -> Self {
        Self::new(None)
    }
}
impl MangaworldConnector {
    pub fn new(browser: Option<Arc<BrowserManager>>) -> Self {
        Self { enabled: true, browser }
    }
}

const DOMAINS: &[&str] = &[
    "https://mangaworld.cx",
    "https://mangaworld.bz",
    "https://mangaworld.fun",
    "https://mangaworld.ac",
    "https://mangaworld.mx",
];

#[async_trait]
impl MangaConnector for MangaworldConnector {
    fn name(&self) -> &str { "Mangaworld" }
    fn supported_languages(&self) -> &[&str] { &["it", "en"] }
    fn base_uris(&self) -> &[&str] { DOMAINS }
    fn icon_url(&self) -> &str { "https://mangaworld.cx/favicon.ico" }
    fn enabled(&self) -> bool { self.enabled }
    fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }

    async fn search_manga(&self, query: &str, http: &dyn HttpClient) -> Result<Vec<NewMangaResult>, anyhow::Error> {
        for domain in DOMAINS {
            let url = format!("{domain}/archive?keyword={query}");
            let resp = http.make_request(&url, RequestType::Default, None, CancellationToken::new()).await;
            if let Ok(resp) = resp
                && (200..300).contains(&resp.status) {
                    return Ok(parse_mangaworld_search(&resp.body, domain));
                }
        }
        Ok(vec![])
    }

    async fn get_manga_from_url(&self, url: &str, http: &dyn HttpClient) -> Result<Option<NewMangaResult>, anyhow::Error> {
        if !self.url_matches_connector(url) {
            return Ok(None);
        }

        let re = regex_lazy();
        let site_id = match re.captures(url) {
            Some(c) => format!("{}/{}", c.get(1).unwrap().as_str(), c.get(2).unwrap().as_str()),
            None => url.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string(),
        };

        let resp = http.make_request(url, RequestType::Default, None, CancellationToken::new()).await?;
        if !(200..300).contains(&resp.status) {
            return Ok(None);
        }

        Ok(Some(parse_mangaworld_manga(&resp.body, &site_id, url)?))
    }

    async fn get_chapters(&self, connector_site_id: &str, _language: Option<&str>, _http: &dyn HttpClient) -> Result<Vec<NewChapterResult>, anyhow::Error> {
        let browser = self.browser.as_ref().ok_or_else(|| anyhow::anyhow!("chromium not available"))?;
        for domain in DOMAINS {
            let url = format!("{domain}/manga/{connector_site_id}");
            if let Ok(html) = browser.fetch_html(&url).await {
                let document = scraper::Html::parse_document(&html);
                let chapter_sel = match scraper::Selector::parse("a.chapter") {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let mut chapters = Vec::new();
                for element in document.select(&chapter_sel) {
                    let href = element.value().attr("href").unwrap_or("").to_string();
                    let text = element.text().collect::<Vec<_>>().join("").trim().to_string();

                    let ch_num = text.split_whitespace().last().unwrap_or("").to_string();
                    let ch_num = crate::utils::normalize_chapter_number(&ch_num);

                    if ch_num.is_empty() { continue; }

                    chapters.push(NewChapterResult {
                        chapter_number: ch_num,
                        volume_number: None,
                        title: Some(text),
                        connector_site_id: href.trim_start_matches('/').to_string(),
                        website_url: Some(format!("{domain}{href}")),
                    });
                }

                if !chapters.is_empty() {
                    chapters.reverse();
                    return Ok(chapters);
                }
            }
        }
        Ok(vec![])
    }

    async fn get_chapter_image_urls(&self, connector_site_id: &str, _http: &dyn HttpClient) -> Result<Vec<String>, anyhow::Error> {
        let browser = self.browser.as_ref().ok_or_else(|| anyhow::anyhow!("chromium not available"))?;
        for domain in DOMAINS {
            let url = format!("{domain}/{connector_site_id}");
            if let Ok(html) = browser.fetch_html(&url).await {
                let base = url::Url::parse(&url).unwrap_or_else(|_| url::Url::parse(domain).unwrap());
                let urls = extract_mangaworld_image_urls(&html, &base);
                if !urls.is_empty() {
                    return Ok(urls);
                }
            }
        }
        Ok(vec![])
    }

    fn url_matches_connector(&self, url: &str) -> bool {
        DOMAINS.iter().any(|d| url.contains(d))
    }
}

fn make_absolute_url(url: &str, base: &str) -> String {
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

fn regex_lazy() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"https?://[^/]+/manga/(\d+)/([^/]+)").unwrap())
}

fn parse_mangaworld_manga(html: &str, site_id: &str, url: &str) -> Result<NewMangaResult, anyhow::Error> {
    let document = scraper::Html::parse_document(html);

    let title = scraper::Selector::parse("meta[property='og:title']")
        .ok()
        .and_then(|s| document.select(&s).next())
        .and_then(|e| e.value().attr("content"))
        .map(|t| t.to_string())
        .or_else(|| {
            scraper::Selector::parse("h1").ok()
                .and_then(|s| document.select(&s).next())
                .and_then(|e| e.text().next())
                .map(|t| t.trim().to_string())
        })
        .unwrap_or_default();

    let title = clean_mangaworld_title(&title);

    let cover_url = scraper::Selector::parse("meta[property='og:image']")
        .ok()
        .and_then(|s| document.select(&s).next())
        .and_then(|e| e.value().attr("content"))
        .or_else(|| {
            scraper::Selector::parse("div.cover img, div[class*='cover'] img").ok()
                .and_then(|s| document.select(&s).next())
                .and_then(|e| e.value().attr("src"))
        })
        .unwrap_or("")
        .to_string();
    let cover_url = make_absolute_url(&cover_url, url);

    let description = scraper::Selector::parse("meta[name='description']")
        .ok()
        .and_then(|s| document.select(&s).next())
        .and_then(|e| e.value().attr("content"))
        .map(|t| t.trim().to_string())
        .or_else(|| {
            scraper::Selector::parse("div.description, div.trama").ok()
                .and_then(|s| document.select(&s).next())
                .and_then(|e| e.text().next())
                .map(|t| t.trim().to_string())
        })
        .unwrap_or_default();

    let release_status = scraper::Selector::parse("span")
        .ok()
        .and_then(|s| {
            document.select(&s).find(|e| e.text().collect::<String>().trim() == "Stato:")
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|e| e.text().next().map(|t| map_italian_status(t.trim())))
        .unwrap_or("Continuing")
        .to_string();

    let tags = extract_mangaworld_sibling_texts_by_label(&document, "Generi:");

    Ok(NewMangaResult {
        name: title,
        description,
        cover_url,
        release_status,
        authors: vec![],
        tags,
        links: vec![],
        alt_titles: vec![],
        connector_site_id: site_id.to_string(),
        website_url: Some(url.to_string()),
        year: None,
        original_language: Some("it".into()),
    })
}

fn extract_mangaworld_sibling_texts_by_label(document: &scraper::Html, label: &str) -> Vec<String> {
    scraper::Selector::parse("span")
        .ok()
        .and_then(|s| {
            document.select(&s).find(|e| e.text().collect::<String>().trim() == label)
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|container| {
            let a_sel = scraper::Selector::parse("a").ok()?;
            Some(container.select(&a_sel).map(|e| e.text().collect::<String>()).collect::<Vec<_>>())
        })
        .map(|v| v.into_iter().filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

fn clean_mangaworld_title(title: &str) -> String {
    let re = regex::Regex::new(r"\s*Scan\s+\S+\s*-\s*MangaWorld\s*$").ok();
    match re {
        Some(r) => r.replace(title, "").to_string(),
        None => title.to_string(),
    }
}

fn map_italian_status(status: &str) -> &'static str {
    match status.to_lowercase().as_str() {
        "in corso" | "ongoing" => "Continuing",
        "completo" | "concluso" | "finito" => "Completed",
        "in pausa" | "hiatus" => "OnHiatus",
        "droppato" | "cancellato" | "interrotto" => "Cancelled",
        _ => "Continuing",
    }
}

fn extract_mangaworld_image_urls(html: &str, base: &url::Url) -> Vec<String> {
    let document = scraper::Html::parse_document(html);
    let img_sel = match scraper::Selector::parse("img") {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let valid_exts = [".jpg", ".jpeg", ".png", ".webp"];
    let mut urls = Vec::new();

    for img in document.select(&img_sel) {
        let attrs = [
            img.value().attr("data-src").map(|s| s.to_string()),
            img.value().attr("src").map(|s| s.to_string()),
        ];
        let srcset = img.value().attr("srcset").map(|s| {
            s.split(',')
                .filter_map(|part| part.split_whitespace().next())
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        }).unwrap_or_default();

        for raw in attrs.into_iter().flatten().chain(srcset) {
            if raw.is_empty() { continue; }
            let abs = make_absolute_url_from_base(base, &raw);
            if !abs.starts_with("http") { continue; }
            let lower = abs.to_lowercase();
            if valid_exts.iter().any(|ext| lower.ends_with(ext)) {
                urls.push(abs);
            }
        }
    }

    urls.sort();
    urls.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    urls
}

fn make_absolute_url_from_base(base: &url::Url, raw: &str) -> String {
    if raw.is_empty() { return raw.to_string(); }
    let trimmed = raw.trim();
    if trimmed.starts_with("//") {
        format!("https:{}", trimmed)
    } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        base.join(trimmed).map(|u| u.to_string()).unwrap_or_else(|_| trimmed.to_string())
    }
}

fn parse_mangaworld_search(html: &str, base_url: &str) -> Vec<NewMangaResult> {
    let document = scraper::Html::parse_document(html);
    let sel = match scraper::Selector::parse("div.archive-entry") {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    document.select(&sel).filter_map(|el| {
        let name_sel = scraper::Selector::parse("a.entry-title").ok()?;
        let name = el.select(&name_sel).next()?.text().next()?.trim().to_string();
        let href = el.select(&name_sel).next()?.value().attr("href")?.to_string();
        let site_id = href.trim_start_matches('/').to_string();
        Some(NewMangaResult {
            name, description: String::new(), cover_url: String::new(),
            release_status: "Continuing".into(),
            authors: vec![], tags: vec![], links: vec![], alt_titles: vec![],
            connector_site_id: site_id,
            website_url: Some(format!("{base_url}/{href}")),
            year: None, original_language: Some("it".into()),
        })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mangaworld_search_extracts_entries() {
        let html = include_str!("fixtures/mangaworld_search.html");
        let results = parse_mangaworld_search(html, "https://mangaworld.cx");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "Dragon Ball");
        assert_eq!(results[1].name, "Naruto");
    }

    #[test]
    fn url_matches_mangaworld_domains() {
        let connector = MangaworldConnector::new(None);
        assert!(connector.url_matches_connector("https://mangaworld.cx/manga/db"));
        assert!(connector.url_matches_connector("https://mangaworld.bz/manga/db"));
        assert!(!connector.url_matches_connector("https://example.com"));
    }
}
