//! Pure HTML → domain parsing, kept apart from IO so it is easy to test
//! against saved fixtures.
//!
//! `scraper::Html` is not `Send`, so it must never live across an
//! `.await`; keeping parsing in sync functions guarantees that.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use kaze_core::{
    ChapterInfo, ChapterKey, ChapterNumber, Result, Series, SeriesHit, SeriesKey, SeriesStatus,
    SourceError,
};
use scraper::{ElementRef, Html};
use url::Url;

use crate::{
    html::{sel, text},
    literal::re,
};

/// WeebCentral ids are ULIDs: 26 chars of Crockford base32.
pub fn is_ulid(s: &str) -> bool {
    s.len() == 26
        && s.bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
}

/// The ULID after `/series/` or `/chapters/` in a link.
fn id_after<'a>(href: &'a str, segment: &str) -> Option<&'a str> {
    let rest = &href[href.find(segment)? + segment.len()..];
    let id = rest.split(['/', '?', '#']).next()?;
    is_ulid(id).then_some(id)
}

pub fn search(html: &str, base: &Url) -> Result<Vec<SeriesHit>> {
    let doc = Html::parse_fragment(html);
    let mut seen = HashSet::new();
    let mut hits = Vec::new();
    for card in doc.select(sel!("article.bg-base-300")) {
        let Some(href) = card
            .select(sel!("a[href*='/series/']"))
            .find_map(|a| a.value().attr("href"))
        else {
            continue;
        };
        let Some(id) = id_after(href, "/series/") else {
            continue;
        };
        if !seen.insert(id.to_owned()) {
            continue;
        }
        let cover = card.select(sel!("img")).next();
        let title = card
            .select(sel!("a.line-clamp-1"))
            .next()
            .and_then(text)
            .or_else(|| {
                cover
                    .and_then(|img| img.value().attr("alt"))
                    .map(strip_cover_suffix)
            });
        let Some(title) = title else { continue };
        hits.push(SeriesHit {
            key: id.into(),
            title,
            url: join(base, href)?,
            cover_url: cover
                .and_then(|img| img.value().attr("src"))
                .and_then(|src| base.join(src).ok()),
        });
    }
    Ok(hits)
}

pub fn series(html: &str, key: &SeriesKey, url: &Url) -> Result<Series> {
    let doc = Html::parse_document(html);

    let title = doc
        .select(sel!("h1"))
        .next()
        .and_then(text)
        .or_else(|| {
            let title = doc.select(sel!("title")).next().and_then(text)?;
            Some(title.split(" | ").next()?.to_owned())
        })
        .ok_or_else(|| SourceError::parse("series page", "no title"))?;

    let mut series = Series {
        key: key.clone(),
        title,
        alt_titles: Vec::new(),
        url: doc
            .select(sel!("link[rel='canonical']"))
            .next()
            .and_then(|l| l.value().attr("href"))
            .and_then(|href| url.join(href).ok())
            .unwrap_or_else(|| url.clone()),
        cover_url: doc
            .select(sel!("meta[property='og:image']"))
            .next()
            .and_then(|m| m.value().attr("content"))
            .and_then(|c| url.join(c).ok()),
        description: None,
        authors: Vec::new(),
        tags: Vec::new(),
        status: SeriesStatus::Unknown,
        year: None,
        language: Some("en".to_owned()),
    };

    // Metadata is a list of `<li><strong>Label: </strong> value…</li>`.
    for label in doc.select(sel!("li > strong")) {
        let Some(item) = label.parent().and_then(ElementRef::wrap) else {
            continue;
        };
        let name = text(label).unwrap_or_default();
        let links = || item.select(sel!("a")).filter_map(text).collect::<Vec<_>>();
        match name.trim_end_matches([':', ' ']) {
            "Author(s)" => series.authors = links(),
            "Tags(s)" | "Tag(s)" => series.tags = links(),
            "Status" => {
                series.status = links().first().map_or(SeriesStatus::Unknown, |s| status(s));
            }
            "Released" => {
                series.year = item
                    .select(sel!("span"))
                    .next()
                    .and_then(text)
                    .and_then(|y| y.parse().ok());
            }
            "Description" => series.description = item.select(sel!("p")).next().and_then(text),
            _ => {}
        }
    }
    Ok(series)
}

fn status(s: &str) -> SeriesStatus {
    match s.to_ascii_lowercase().as_str() {
        "ongoing" => SeriesStatus::Ongoing,
        "complete" | "completed" => SeriesStatus::Completed,
        "hiatus" => SeriesStatus::Hiatus,
        "canceled" | "cancelled" => SeriesStatus::Cancelled,
        _ => SeriesStatus::Unknown,
    }
}

pub fn chapters(html: &str, base: &Url) -> Result<Vec<ChapterInfo>> {
    let doc = Html::parse_fragment(html);
    let mut chapters = Vec::new();
    for link in doc.select(sel!("a[href*='/chapters/']")) {
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let Some(id) = id_after(href, "/chapters/") else {
            continue;
        };
        // The label is the only `<span class="">`; fall back to all text.
        let Some(label) = link
            .select(sel!("span[class='']"))
            .next()
            .and_then(text)
            .or_else(|| text(link))
        else {
            continue;
        };
        let Some(parsed) = parse_label(&label) else {
            tracing::debug!(%label, "skipping chapter without a number");
            continue;
        };
        chapters.push(ChapterInfo {
            key: ChapterKey::from(id),
            number: parsed.number,
            volume: parsed.volume,
            title: parsed.title,
            url: join(base, href)?,
            language: Some("en".to_owned()),
            scanlator: None,
            published_at: link
                .select(sel!("time[datetime]"))
                .next()
                .and_then(|t| t.value().attr("datetime"))
                .and_then(|d| DateTime::parse_from_rfc3339(d).ok())
                .map(|d| d.with_timezone(&Utc)),
        });
    }
    Ok(chapters)
}

pub fn pages(html: &str) -> Result<Vec<Url>> {
    let doc = Html::parse_fragment(html);
    let urls: Vec<Url> = doc
        .select(sel!("img[alt^='Page']"))
        .filter_map(|img| {
            img.value()
                .attr("src")
                .or_else(|| img.value().attr("data-src"))
        })
        .filter_map(|src| Url::parse(src).ok())
        .collect();
    if urls.is_empty() {
        return Err(SourceError::parse("chapter images", "no page images found"));
    }
    Ok(urls)
}

pub fn latest_updates(html: &str) -> Vec<SeriesKey> {
    let doc = Html::parse_fragment(html);
    let mut seen = HashSet::new();
    doc.select(sel!("article a[href*='/series/']"))
        .filter_map(|a| id_after(a.value().attr("href")?, "/series/"))
        .filter(|id| seen.insert(*id))
        .map(SeriesKey::from)
        .collect()
}

fn join(base: &Url, href: &str) -> Result<Url> {
    base.join(href)
        .map_err(|e| SourceError::parse("link", format!("{href}: {e}")))
}

fn strip_cover_suffix(alt: &str) -> String {
    alt.trim_end_matches(" cover").to_owned()
}

#[derive(Debug, PartialEq, Eq)]
struct Label {
    number: ChapterNumber,
    volume: Option<u32>,
    title: Option<String>,
}

/// Turn a chapter label into a number, volume and title.
///
/// * `Chapter 12`, `Ch. 12.5`, `episode. 36` → plain numbered chapters.
/// * `Vol. 3 Chapter 12` → also a volume.
/// * `Chapter 12: The End` → keeps the trailing text as the title.
/// * `Prologue 3` (Berserk) → number 3, but the label is kept as the title
///   so it does not overwrite `Chapter 3` on disk.
fn parse_label(label: &str) -> Option<Label> {
    let volume_re = re!(r"(?i)^\s*(?:volume|vol\.?|season)\s*(\d+)[\s:,-]*");
    let chapter_re =
        re!(r"(?i)^(?:chapter|chap|ch|episode|ep)\.?\s*(\d+(?:[.,]\d+)?)\s*[:\-–]?\s*(.*)$");
    let any_number = re!(r"\d+(?:[.,]\d+)?");

    let label = label.trim();
    let (volume, rest) = match volume_re.captures(label) {
        Some(c) => (c[1].parse().ok(), &label[c.get(0)?.end()..]),
        None => (None, label),
    };

    if let Some(c) = chapter_re.captures(rest) {
        let title = c[2].trim();
        return Some(Label {
            number: c[1].parse().ok()?,
            volume,
            title: (!title.is_empty()).then(|| title.to_owned()),
        });
    }

    let number = any_number.find_iter(rest).last()?.as_str().parse().ok()?;
    Some(Label {
        number,
        volume,
        title: Some(rest.to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://weebcentral.com").unwrap()
    }

    fn label(s: &str) -> (String, Option<u32>, Option<String>) {
        let l = parse_label(s).unwrap();
        (l.number.to_string(), l.volume, l.title)
    }

    #[test]
    fn chapter_labels() {
        assert_eq!(label("Chapter 1195"), ("1195".into(), None, None));
        assert_eq!(label("Chapter 10.5"), ("10.5".into(), None, None));
        assert_eq!(label("episode. 36"), ("36".into(), None, None));
        assert_eq!(label("Ch.7"), ("7".into(), None, None));
        assert_eq!(label("Vol. 2 Chapter 9"), ("9".into(), Some(2), None));
        assert_eq!(
            label("Chapter 3: Black Swordsman"),
            ("3".into(), None, Some("Black Swordsman".into()))
        );
        assert_eq!(
            label("Prologue 3"),
            ("3".into(), None, Some("Prologue 3".into()))
        );
        assert!(parse_label("Oneshot").is_none());
    }

    #[test]
    fn parses_search_results() {
        let hits = search(include_str!("fixtures/search.html"), &base()).unwrap();
        assert_eq!(hits.len(), 5);
        let first = &hits[0];
        assert_eq!(first.key.as_str(), "01J76XY7E9FNDZ1DBBM6PBJPFK");
        assert_eq!(first.title, "One Piece");
        assert_eq!(
            first.url.as_str(),
            "https://weebcentral.com/series/01J76XY7E9FNDZ1DBBM6PBJPFK/One-Piece"
        );
        assert_eq!(
            first.cover_url.as_ref().unwrap().as_str(),
            "https://temp.compsci88.com/cover/fallback/01J76XY7E9FNDZ1DBBM6PBJPFK.jpg"
        );
        let keys: HashSet<_> = hits.iter().map(|h| &h.key).collect();
        assert_eq!(keys.len(), hits.len(), "no duplicates");
    }

    #[test]
    fn parses_series_page() {
        let key = SeriesKey::from("01J76XY7E9FNDZ1DBBM6PBJPFK");
        let url = base().join("/series/01J76XY7E9FNDZ1DBBM6PBJPFK").unwrap();
        let s = series(include_str!("fixtures/series_one_piece.html"), &key, &url).unwrap();
        assert_eq!(s.title, "One Piece");
        assert_eq!(
            s.url.as_str(),
            "https://weebcentral.com/series/01J76XY7E9FNDZ1DBBM6PBJPFK/One-Piece"
        );
        assert_eq!(s.authors, ["ODA Eiichiro"]);
        assert_eq!(
            s.tags,
            [
                "Action",
                "Adventure",
                "Comedy",
                "Drama",
                "Fantasy",
                "Shounen"
            ]
        );
        assert_eq!(s.status, SeriesStatus::Ongoing);
        assert_eq!(s.year, Some(1997));
        assert!(
            s.description
                .unwrap()
                .starts_with("As a child, Monkey D. Luffy")
        );
        assert!(
            s.cover_url
                .unwrap()
                .as_str()
                .ends_with("01J76XY7E9FNDZ1DBBM6PBJPFK.jpg")
        );
    }

    #[test]
    fn parses_chapter_list() {
        let list = chapters(include_str!("fixtures/chapters_one_piece.html"), &base()).unwrap();
        assert_eq!(list.len(), 4);
        let newest = &list[0];
        assert_eq!(newest.key.as_str(), "01M4FFVTBPS4RKXQWZ2REYGH3M");
        assert_eq!(newest.number.to_string(), "1195");
        assert_eq!(newest.title, None);
        assert_eq!(
            newest.url.as_str(),
            "https://weebcentral.com/chapters/01M4FFVTBPS4RKXQWZ2REYGH3M"
        );
        assert_eq!(
            newest.published_at.unwrap().to_rfc3339(),
            "2026-10-09T04:48:39.286+00:00"
        );
    }

    #[test]
    fn prologues_keep_their_label() {
        let list = chapters(include_str!("fixtures/chapters_berserk.html"), &base()).unwrap();
        let labels: Vec<_> = list
            .iter()
            .map(|c| (c.number.to_string(), c.title.clone()))
            .collect();
        assert!(labels.contains(&("1".into(), None)));
        assert!(labels.contains(&("1".into(), Some("Prologue 1".into()))));
        assert!(labels.contains(&("16".into(), Some("Prologue 16".into()))));
    }

    #[test]
    fn parses_page_images() {
        let urls = pages(include_str!("fixtures/images.html")).unwrap();
        assert_eq!(urls.len(), 16);
        assert_eq!(
            urls[0].as_str(),
            "https://temp.compsci88.com/manga/One-Piece/1195-001.png"
        );
        assert_eq!(
            urls[15].as_str(),
            "https://temp.compsci88.com/manga/One-Piece/1195-016.png"
        );
    }

    #[test]
    fn empty_images_fragment_is_an_error() {
        assert!(matches!(
            pages("<section></section>"),
            Err(SourceError::Parse { .. })
        ));
    }

    #[test]
    fn parses_latest_updates() {
        let keys = latest_updates(include_str!("fixtures/latest_updates.html"));
        assert!(keys.len() >= 20, "{}", keys.len());
        assert_eq!(keys[0].as_str(), "01M3SBE5EKTB7HGB94AZXY8MTK");
    }
}
