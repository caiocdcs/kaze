//! The parts of AniList's GraphQL API that kaze reads, and their mapping to
//! kaze's types. See <https://docs.anilist.co>.

use kaze_core::{AniListId, SeriesStatus};
use serde::{Deserialize, Serialize};
use url::Url;

/// Fields read for every media, in search results and by id.
const FIELDS: &str = "id format popularity status \
    title { romaji english native } synonyms description(asHtml: false) genres \
    tags { name rank isMediaSpoiler } startDate { year } coverImage { extraLarge large } \
    staff(perPage: 25, sort: [RELEVANCE]) { edges { role node { name { full } } } }";

/// Tags AniList's voters rank below this (0–100) are noise.
const MIN_TAG_RANK: u8 = 60;

pub fn search_query() -> String {
    format!(
        "query ($search: String) {{ Page(perPage: 10) {{ \
         media(search: $search, type: MANGA, format_not_in: [NOVEL]) {{ {FIELDS} }} }} }}"
    )
}

pub fn media_query() -> String {
    format!("query ($id: Int) {{ Media(id: $id, type: MANGA) {{ {FIELDS} }} }}")
}

#[derive(Debug, Serialize)]
pub struct Request<'a, V> {
    pub query: &'a str,
    pub variables: V,
}

#[derive(Debug, Deserialize)]
pub struct Response<T> {
    pub data: T,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PageData {
    pub page: Page,
}

#[derive(Debug, Deserialize)]
pub struct Page {
    pub media: Vec<RawMedia>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaData {
    pub media: RawMedia,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMedia {
    id: u32,
    format: Option<Format>,
    popularity: Option<u32>,
    status: Option<String>,
    title: Titles,
    #[serde(default)]
    synonyms: Vec<String>,
    description: Option<String>,
    #[serde(default)]
    genres: Vec<String>,
    #[serde(default)]
    tags: Vec<RawTag>,
    start_date: Option<FuzzyDate>,
    cover_image: Option<CoverImage>,
    staff: Option<Connection<StaffEdge>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Titles {
    pub romaji: Option<String>,
    pub english: Option<String>,
    pub native: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Format {
    Manga,
    OneShot,
    Novel,
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTag {
    name: String,
    rank: Option<u8>,
    #[serde(default)]
    is_media_spoiler: bool,
}

#[derive(Debug, Deserialize)]
struct FuzzyDate {
    year: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoverImage {
    extra_large: Option<Url>,
    large: Option<Url>,
}

#[derive(Debug, Deserialize)]
struct Connection<T> {
    edges: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct StaffEdge {
    role: Option<String>,
    node: StaffNode,
}

#[derive(Debug, Deserialize)]
struct StaffNode {
    name: StaffName,
}

#[derive(Debug, Deserialize)]
struct StaffName {
    full: Option<String>,
}

/// What a staff member did, for ComicInfo's `Writer` and `Penciller`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Story,
    Art,
    StoryAndArt,
}

impl Role {
    /// AniList roles are free text: "Story & Art", "Original Story",
    /// "Art (eps 1-3)", "Translator (English)"… Only creators count.
    fn parse(role: &str) -> Option<Self> {
        let role = role.to_lowercase();
        if role.contains("assistant") {
            return None;
        }
        let story = role.starts_with("story")
            || role.starts_with("original story")
            || role.starts_with("original creator");
        let art = role.starts_with("art") || role.contains("& art");
        match (story, art) {
            (true, true) => Some(Self::StoryAndArt),
            (true, false) => Some(Self::Story),
            (false, true) => Some(Self::Art),
            (false, false) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staff {
    pub name: String,
    pub role: Role,
}

/// A manga on AniList, with the fields kaze uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    pub id: AniListId,
    pub format: Option<Format>,
    pub popularity: u32,
    pub titles: Titles,
    pub synonyms: Vec<String>,
    /// Plain text: AniList's markup is stripped.
    pub description: Option<String>,
    pub genres: Vec<String>,
    /// Non-spoiler tags ranked at least [`MIN_TAG_RANK`].
    pub tags: Vec<String>,
    pub status: SeriesStatus,
    pub year: Option<u16>,
    pub cover_url: Option<Url>,
    pub staff: Vec<Staff>,
}

impl From<RawMedia> for Media {
    fn from(raw: RawMedia) -> Self {
        let staff = raw
            .staff
            .map(|s| s.edges)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|edge| {
                Some(Staff {
                    name: edge.node.name.full?,
                    role: Role::parse(edge.role.as_deref()?)?,
                })
            })
            .collect();
        Self {
            id: AniListId(raw.id),
            format: raw.format,
            popularity: raw.popularity.unwrap_or_default(),
            titles: raw.title,
            synonyms: raw.synonyms,
            description: raw
                .description
                .as_deref()
                .map(plain_text)
                .filter(|d| !d.is_empty()),
            genres: raw.genres,
            tags: raw
                .tags
                .into_iter()
                .filter(|t| !t.is_media_spoiler && t.rank.unwrap_or_default() >= MIN_TAG_RANK)
                .map(|t| t.name)
                .collect(),
            status: match raw.status.as_deref() {
                Some("RELEASING") => SeriesStatus::Ongoing,
                Some("FINISHED") => SeriesStatus::Completed,
                Some("HIATUS") => SeriesStatus::Hiatus,
                Some("CANCELLED") => SeriesStatus::Cancelled,
                _ => SeriesStatus::Unknown,
            },
            year: raw.start_date.and_then(|d| d.year),
            cover_url: raw.cover_image.and_then(|c| c.extra_large.or(c.large)),
            staff,
        }
    }
}

/// AniList descriptions carry `<br>`, `<i>` and the odd entity even with
/// `asHtml: false`.
fn plain_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            rest = &rest[start..];
            break;
        };
        let tag = rest[start + 1..start + end].trim().to_lowercase();
        if tag.starts_with("br") {
            out.push('\n');
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    let out = out
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    // `<br>` after a newline doubles every paragraph break.
    let mut lines: Vec<&str> = Vec::new();
    for line in out.lines().map(str::trim_end) {
        if !(line.is_empty() && lines.last().is_some_and(|l| l.is_empty())) {
            lines.push(line);
        }
    }
    lines.join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_markup() {
        assert_eq!(
            plain_text("An <i>elf</i> &amp; a mage.<br>\n<br>\n(Source: Viz)"),
            "An elf & a mage.\n\n(Source: Viz)"
        );
        assert_eq!(plain_text("a < b"), "a < b");
    }

    #[test]
    fn parses_creator_roles() {
        assert_eq!(Role::parse("Story & Art"), Some(Role::StoryAndArt));
        assert_eq!(Role::parse("Original Story"), Some(Role::Story));
        assert_eq!(Role::parse("Original Creator"), Some(Role::Story));
        assert_eq!(Role::parse("Art"), Some(Role::Art));
        assert_eq!(Role::parse("Art Assistant"), None);
        assert_eq!(Role::parse("Translator (English)"), None);
        assert_eq!(Role::parse("Lettering"), None);
    }
}
