//! The parts of MangaDex's JSON API that kaze reads, and their mapping to
//! kaze's types. See <https://api.mangadex.org/docs/>.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use kaze_core::{ChapterInfo, ChapterNumber, Series, SeriesHit, SeriesStatus};
use serde::Deserialize;
use url::Url;

use crate::literal::url;

/// Language-tagged text: `{"en": "...", "ja-ro": "..."}`.
type Localized = BTreeMap<String, String>;

#[derive(Debug, Deserialize)]
pub struct Collection<T> {
    pub data: Vec<T>,
    #[serde(default)]
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct Entity<T> {
    pub data: T,
}

#[derive(Debug, Deserialize)]
pub struct Relationship {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// Present for types requested with `includes[]`.
    #[serde(default)]
    pub attributes: Option<serde_json::Value>,
}

impl Relationship {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.as_ref()?.get(name)?.as_str()
    }
}

#[derive(Debug, Deserialize)]
pub struct Manga {
    pub id: String,
    pub attributes: MangaAttributes,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MangaAttributes {
    pub title: Localized,
    #[serde(default)]
    pub alt_titles: Vec<Localized>,
    #[serde(default)]
    pub description: Localized,
    pub status: Option<String>,
    pub year: Option<u16>,
    #[serde(default)]
    pub tags: Vec<Tag>,
}

#[derive(Debug, Deserialize)]
pub struct Tag {
    pub attributes: TagAttributes,
}

#[derive(Debug, Deserialize)]
pub struct TagAttributes {
    pub name: Localized,
}

#[derive(Debug, Deserialize)]
pub struct Chapter {
    pub id: String,
    pub attributes: ChapterAttributes,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterAttributes {
    pub chapter: Option<String>,
    pub volume: Option<String>,
    pub title: Option<String>,
    pub translated_language: Option<String>,
    /// Set when the chapter is only a link to the official publisher.
    pub external_url: Option<String>,
    #[serde(default)]
    pub pages: u32,
    pub readable_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtHome {
    pub base_url: Url,
    pub chapter: AtHomeChapter,
}

#[derive(Debug, Deserialize)]
pub struct AtHomeChapter {
    pub hash: String,
    pub data: Vec<String>,
}

const SITE: &str = "https://mangadex.org";
const UPLOADS: &str = "https://uploads.mangadex.org";

impl Manga {
    pub fn url(&self) -> Url {
        let mut url = url!(SITE);
        url.set_path(&format!("/title/{}", self.id));
        url
    }

    /// The English title when there is one: that's what users search for
    /// and what the folder will be called.
    pub fn title(&self) -> String {
        let a = &self.attributes;
        a.title
            .get("en")
            .or_else(|| a.alt_titles.iter().find_map(|t| t.get("en")))
            .or_else(|| a.title.values().next())
            .cloned()
            .unwrap_or_else(|| self.id.clone())
    }

    pub fn cover_url(&self) -> Option<Url> {
        let file = self
            .relationships
            .iter()
            .find(|r| r.kind == "cover_art")?
            .attr("fileName")?;
        Url::parse(&format!("{UPLOADS}/covers/{}/{file}", self.id)).ok()
    }

    pub fn hit(&self) -> SeriesHit {
        SeriesHit {
            key: self.id.clone().into(),
            title: self.title(),
            url: self.url(),
            cover_url: self.cover_url(),
        }
    }

    pub fn series(&self, language: &str) -> Series {
        let a = &self.attributes;
        let title = self.title();
        let mut alt_titles: Vec<String> = a
            .title
            .values()
            .chain(a.alt_titles.iter().flat_map(BTreeMap::values))
            .filter(|t| **t != title)
            .cloned()
            .collect();
        alt_titles.dedup();
        let mut authors: Vec<String> = Vec::new();
        for r in &self.relationships {
            if matches!(r.kind.as_str(), "author" | "artist")
                && let Some(name) = r.attr("name")
                && !authors.iter().any(|a| a == name)
            {
                authors.push(name.to_owned());
            }
        }
        Series {
            key: self.id.clone().into(),
            title,
            alt_titles,
            url: self.url(),
            cover_url: self.cover_url(),
            description: a
                .description
                .get("en")
                .or_else(|| a.description.values().next())
                .filter(|d| !d.trim().is_empty())
                .cloned(),
            authors,
            tags: a
                .tags
                .iter()
                .filter_map(|t| t.attributes.name.get("en").cloned())
                .collect(),
            status: match a.status.as_deref() {
                Some("ongoing") => SeriesStatus::Ongoing,
                Some("completed") => SeriesStatus::Completed,
                Some("hiatus") => SeriesStatus::Hiatus,
                Some("cancelled") => SeriesStatus::Cancelled,
                _ => SeriesStatus::Unknown,
            },
            year: a.year,
            language: Some(language.to_owned()),
        }
    }
}

impl Chapter {
    /// `None` for chapters kaze can't download: links to an official site,
    /// or uploads without pages.
    pub fn info(&self) -> Option<ChapterInfo> {
        let a = &self.attributes;
        if a.external_url.is_some() || a.pages == 0 {
            return None;
        }
        // Oneshots have no chapter number.
        let number = match a.chapter.as_deref() {
            Some(n) => n.parse().ok()?,
            None => ChapterNumber::default(),
        };
        let scanlator = self
            .relationships
            .iter()
            .filter(|r| r.kind == "scanlation_group")
            .find_map(|r| r.attr("name"))
            .map(ToOwned::to_owned);
        Some(ChapterInfo {
            key: self.id.clone().into(),
            number,
            volume: a.volume.as_deref().and_then(|v| v.parse().ok()),
            title: a.title.clone().filter(|t| !t.trim().is_empty()),
            url: Url::parse(&format!("{SITE}/chapter/{}", self.id)).ok()?,
            language: a.translated_language.clone(),
            scanlator,
            published_at: a.readable_at,
        })
    }
}

/// Several groups often upload the same chapter; keep one per number
/// (the one with the most pages) so files don't collide.
pub fn one_per_number(chapters: Vec<(ChapterInfo, u32)>) -> Vec<ChapterInfo> {
    let mut best: HashMap<ChapterNumber, (ChapterInfo, u32)> = HashMap::new();
    for (chapter, pages) in chapters {
        match best.get(&chapter.number) {
            Some((_, kept)) if *kept >= pages => {}
            _ => {
                best.insert(chapter.number.clone(), (chapter, pages));
            }
        }
    }
    let mut out: Vec<ChapterInfo> = best.into_values().map(|(c, _)| c).collect();
    out.sort_by(|a, b| a.number.cmp(&b.number));
    out
}
