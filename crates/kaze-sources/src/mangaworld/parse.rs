//! Mangaworld's server-rendered HTML, in Italian.

use chrono::{DateTime, NaiveDate, Utc};
use kaze_core::{
    ChapterInfo, ChapterNumber, Result, Series, SeriesHit, SeriesKey, SeriesStatus, SourceError,
};
use scraper::{ElementRef, Html};
use url::Url;

use crate::html::{sel, text};

/// `/manga/{id}/{slug}` → `"{id}/{slug}"`, the series key.
pub fn series_key(url: &Url) -> Option<String> {
    let mut segments = url.path_segments()?;
    match (segments.next()?, segments.next()?, segments.next()) {
        ("manga", id, Some(slug)) if !slug.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) => {
            Some(format!("{id}/{slug}"))
        }
        _ => None,
    }
}

/// `/manga/{id}/{slug}/read/{hash}` → `"{id}/{slug}/{hash}"`, the chapter key.
pub fn chapter_key(url: &Url) -> Option<String> {
    let segments: Vec<&str> = url.path_segments()?.collect();
    match segments.as_slice() {
        ["manga", id, slug, "read", hash, ..] if id.bytes().all(|b| b.is_ascii_digit()) => {
            Some(format!("{id}/{slug}/{hash}"))
        }
        _ => None,
    }
}

pub fn search(html: &str, base: &Url) -> Vec<SeriesHit> {
    let doc = Html::parse_document(html);
    let mut hits: Vec<SeriesHit> = Vec::new();
    for entry in doc.select(sel!(".entry")) {
        let Some(link) = entry.select(sel!("a.manga-title")).next() else {
            continue;
        };
        let Some(url) = link.value().attr("href").and_then(|h| base.join(h).ok()) else {
            continue;
        };
        let (Some(key), Some(title)) = (series_key(&url), text(link)) else {
            continue;
        };
        if hits.iter().any(|h| h.key.as_str() == key) {
            continue;
        }
        hits.push(SeriesHit {
            key: key.into(),
            title,
            cover_url: entry
                .select(sel!("a.thumb img"))
                .next()
                .and_then(|img| img.value().attr("src"))
                .and_then(|src| base.join(src).ok()),
            url,
        });
    }
    hits
}

/// The values after a "Label: " span in the info box.
fn field<'a>(doc: &'a Html, label: &str) -> Option<ElementRef<'a>> {
    doc.select(sel!(".info .meta-data > div")).find(|div| {
        div.select(sel!("span"))
            .next()
            .and_then(text)
            .is_some_and(|l| l.trim_end_matches(':').trim() == label)
    })
}

fn links(div: Option<ElementRef<'_>>) -> Vec<String> {
    div.map(|d| d.select(sel!("a")).filter_map(text).collect())
        .unwrap_or_default()
}

pub fn series(html: &str, key: &SeriesKey, url: &Url) -> Result<Series> {
    let doc = Html::parse_document(html);
    let title = doc
        .select(sel!(".info h1.name"))
        .next()
        .and_then(text)
        .ok_or_else(|| SourceError::parse("series page", "no title"))?;
    let alt_titles = field(&doc, "Titoli alternativi")
        .and_then(text)
        .and_then(|t| t.split_once(':').map(|(_, rest)| rest.to_owned()))
        .map(|rest| {
            rest.split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let mut authors = links(field(&doc, "Autore"));
    for artist in links(field(&doc, "Artista")) {
        if !authors.contains(&artist) {
            authors.push(artist);
        }
    }
    let status = match links(field(&doc, "Stato")).first().map(String::as_str) {
        Some("In corso") => SeriesStatus::Ongoing,
        Some("Finito") => SeriesStatus::Completed,
        Some("In pausa") => SeriesStatus::Hiatus,
        Some("Cancellato" | "Droppato") => SeriesStatus::Cancelled,
        _ => SeriesStatus::Unknown,
    };
    Ok(Series {
        key: key.clone(),
        title,
        alt_titles,
        url: url.clone(),
        cover_url: doc
            .select(sel!(".thumb img"))
            .next()
            .and_then(|img| img.value().attr("src"))
            .and_then(|src| url.join(src).ok()),
        description: doc.select(sel!("#noidungm")).next().and_then(text),
        authors,
        tags: links(field(&doc, "Generi")),
        status,
        year: links(field(&doc, "Anno di uscita"))
            .first()
            .and_then(|y| y.parse().ok()),
        language: Some("it".into()),
    })
}

/// "Capitolo 12.5" → 12.5
fn chapter_number(label: &str) -> Option<ChapterNumber> {
    label.split_whitespace().find_map(|word| word.parse().ok())
}

/// "08 Ottobre 2026"
fn italian_date(s: &str) -> Option<DateTime<Utc>> {
    const MONTHS: [&str; 12] = [
        "gennaio",
        "febbraio",
        "marzo",
        "aprile",
        "maggio",
        "giugno",
        "luglio",
        "agosto",
        "settembre",
        "ottobre",
        "novembre",
        "dicembre",
    ];
    let mut parts = s.split_whitespace();
    let day: u32 = parts.next()?.parse().ok()?;
    let month = parts.next()?.to_lowercase();
    let month = u32::try_from(MONTHS.iter().position(|m| *m == month)? + 1).ok()?;
    let year: i32 = parts.next()?.parse().ok()?;
    Some(
        NaiveDate::from_ymd_opt(year, month, day)?
            .and_hms_opt(0, 0, 0)?
            .and_utc(),
    )
}

pub fn chapters(html: &str, base: &Url) -> Vec<ChapterInfo> {
    let doc = Html::parse_document(html);
    let mut out: Vec<ChapterInfo> = Vec::new();
    let mut add = |link: ElementRef<'_>, volume: Option<u32>| {
        let Some(url) = link.value().attr("href").and_then(|h| base.join(h).ok()) else {
            return;
        };
        let Some(key) = chapter_key(&url) else {
            return;
        };
        let Some(number) = link
            .select(sel!("span"))
            .next()
            .and_then(text)
            .as_deref()
            .and_then(chapter_number)
        else {
            return;
        };
        if out.iter().any(|c| c.key.as_str() == key) {
            return;
        }
        out.push(ChapterInfo {
            key: key.into(),
            number,
            volume,
            title: None,
            url,
            language: Some("it".into()),
            scanlator: None,
            published_at: link
                .select(sel!(".chap-date"))
                .next()
                .and_then(text)
                .as_deref()
                .and_then(italian_date),
        });
    };
    for volume in doc.select(sel!(".volume-element")) {
        let number = volume
            .select(sel!(".volume-name"))
            .next()
            .and_then(text)
            .and_then(|t| t.split_whitespace().find_map(|w| w.parse().ok()));
        for link in volume.select(sel!("a.chap")) {
            add(link, number);
        }
    }
    // Series without volumes list chapters directly.
    for link in doc.select(sel!(".chapters-wrapper a.chap")) {
        add(link, None);
    }
    out
}

pub fn pages(html: &str, base: &Url) -> Result<Vec<Url>> {
    let doc = Html::parse_document(html);
    let pages: Vec<Url> = doc
        .select(sel!("img.page-image"))
        .filter_map(|img| {
            let v = img.value();
            v.attr("data-src").or_else(|| v.attr("src"))
        })
        .filter_map(|src| base.join(src).ok())
        .collect();
    if pages.is_empty() {
        return Err(SourceError::parse("chapter pages", "no page images"));
    }
    Ok(pages)
}

/// Series linked from the home page, newest releases first.
pub fn latest(html: &str, base: &Url) -> Vec<SeriesKey> {
    let doc = Html::parse_document(html);
    let mut keys: Vec<SeriesKey> = Vec::new();
    for link in doc.select(sel!(r#"a[href*="/manga/"]"#)) {
        let Some(key) = link
            .value()
            .attr("href")
            .and_then(|h| base.join(h).ok())
            .as_ref()
            .and_then(series_key)
        else {
            continue;
        };
        let key = SeriesKey::from(key);
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_italian_dates() {
        let date = italian_date("08 Ottobre 2026").unwrap();
        assert_eq!(date.to_rfc3339(), "2026-10-08T00:00:00+00:00");
        assert_eq!(italian_date("32 Ottobre 2026"), None);
        assert_eq!(italian_date("08 October 2026"), None);
    }

    #[test]
    fn keys_from_urls() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert_eq!(
            series_key(&u("https://www.mangaworld.mx/manga/1708/one-piece/")).as_deref(),
            Some("1708/one-piece")
        );
        assert_eq!(
            chapter_key(&u(
                "https://www.mangaworld.mx/manga/1708/one-piece/read/6ac7bc/1"
            ))
            .as_deref(),
            Some("1708/one-piece/6ac7bc")
        );
        assert_eq!(
            series_key(&u("https://www.mangaworld.mx/archive?x=1")),
            None
        );
    }
}
