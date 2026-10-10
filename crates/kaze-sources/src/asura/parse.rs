//! Asura Scans is an Astro site: the interesting data sits in each
//! `<astro-island props="...">` as JSON, in Astro's own encoding where
//! every value is a `[type, value]` pair. [`decode`] turns that back into
//! plain JSON; the rest is serde.

use chrono::{DateTime, Utc};
use kaze_core::{ChapterInfo, Result, Series, SeriesHit, SeriesKey, SeriesStatus, SourceError};
use scraper::Html;
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::html::{sel, text};

/// Undo Astro's prop encoding: `[0, x]` is a plain value (objects have
/// encoded fields), `[1, [...]]` an array of encoded items, `[0]` is
/// `undefined`. Other tags (dates, maps, ...) keep their raw value.
fn decode(value: &Value) -> Value {
    match value {
        Value::Array(pair) if pair.first().is_some_and(Value::is_u64) => {
            match (pair[0].as_u64(), pair.get(1)) {
                (_, None) => Value::Null,
                (Some(1), Some(Value::Array(items))) => {
                    Value::Array(items.iter().map(decode).collect())
                }
                (Some(0), Some(inner)) => decode_fields(inner),
                (_, Some(raw)) => raw.clone(),
            }
        }
        other => decode_fields(other),
    }
}

fn decode_fields(value: &Value) -> Value {
    match value {
        Value::Object(fields) => {
            Value::Object(fields.iter().map(|(k, v)| (k.clone(), decode(v))).collect())
        }
        other => other.clone(),
    }
}

/// The decoded props of the island whose component file contains `name`.
fn island<T: for<'de> Deserialize<'de>>(html: &str, name: &'static str) -> Result<T> {
    let doc = Html::parse_document(html);
    let props = doc
        .select(sel!("astro-island"))
        .filter(|el| {
            el.value()
                .attr("component-url")
                .is_some_and(|url| url.contains(name))
        })
        .find_map(|el| el.value().attr("props"))
        .ok_or_else(|| SourceError::parse(name, "component not found on the page"))?;
    let raw: Value =
        serde_json::from_str(props).map_err(|e| SourceError::parse(name, e.to_string()))?;
    serde_json::from_value(decode(&raw)).map_err(|e| SourceError::parse(name, e.to_string()))
}

/// Series links end in a site-wide suffix that changes now and then
/// (`solo-leveling-bd5bdaf8`). The bare slug redirects to the current one,
/// so it is the stable key.
pub fn bare_slug(slug: &str) -> &str {
    match slug.rsplit_once('-') {
        Some((base, suffix))
            if suffix.len() == 8 && suffix.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            base
        }
        _ => slug,
    }
}

/// Series cards of the browse page: search results or latest updates.
pub fn cards(html: &str, base: &Url) -> Vec<SeriesHit> {
    let doc = Html::parse_document(html);
    let mut hits: Vec<SeriesHit> = Vec::new();
    for card in doc.select(sel!(".series-card")) {
        let Some(link) = card.select(sel!(r#"a[href^="/comics/"]"#)).next() else {
            continue;
        };
        let Some(slug) = link
            .value()
            .attr("href")
            .and_then(|h| h.strip_prefix("/comics/"))
            .map(bare_slug)
        else {
            continue;
        };
        let image = card.select(sel!("img")).next();
        let title = card.select(sel!("h3")).next().and_then(text).or_else(|| {
            image
                .and_then(|i| i.value().attr("alt"))
                .map(ToOwned::to_owned)
        });
        let (Some(title), Ok(url)) = (title, base.join(&format!("/comics/{slug}"))) else {
            continue;
        };
        if hits.iter().any(|h| h.key.as_str() == slug) {
            continue;
        }
        hits.push(SeriesHit {
            key: slug.into(),
            title,
            url,
            cover_url: image
                .and_then(|i| i.value().attr("src"))
                .and_then(|src| base.join(src).ok()),
        });
    }
    hits
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeriesProps {
    title: String,
    description: Option<String>,
    alternative_titles: Option<String>,
    cover_url: Option<String>,
    status: Option<String>,
    author: Option<String>,
    artist: Option<String>,
    #[serde(default)]
    genres: Vec<Genre>,
}

#[derive(Deserialize)]
struct Genre {
    name: String,
}

pub fn series(html: &str, key: &SeriesKey, url: &Url) -> Result<Series> {
    let d: SeriesProps = island(html, "DescriptionModal")?;
    let mut authors = Vec::new();
    for person in [d.author, d.artist].into_iter().flatten() {
        let person = person.trim().to_owned();
        if !person.is_empty() && !authors.contains(&person) {
            authors.push(person);
        }
    }
    Ok(Series {
        key: key.clone(),
        alt_titles: d
            .alternative_titles
            .unwrap_or_default()
            .split('•')
            .map(str::trim)
            .filter(|t| !t.is_empty() && *t != d.title)
            .map(ToOwned::to_owned)
            .collect(),
        title: d.title,
        url: url.clone(),
        cover_url: d.cover_url.and_then(|c| url.join(&c).ok()),
        // The description is HTML; keep its text.
        description: d.description.and_then(|html| {
            let fragment = Html::parse_fragment(&html);
            text(fragment.root_element())
        }),
        authors,
        tags: d.genres.into_iter().map(|g| g.name).collect(),
        status: match d.status.as_deref() {
            Some("ongoing" | "season end") => SeriesStatus::Ongoing,
            Some("completed") => SeriesStatus::Completed,
            Some("hiatus") => SeriesStatus::Hiatus,
            Some("dropped" | "cancelled") => SeriesStatus::Cancelled,
            _ => SeriesStatus::Unknown,
        },
        year: None,
        language: Some("en".into()),
    })
}

#[derive(Deserialize)]
struct ChapterList {
    chapters: Vec<ChapterEntry>,
}

#[derive(Deserialize)]
struct ChapterEntry {
    number: serde_json::Number,
    title: Option<String>,
    #[serde(default)]
    is_premium: bool,
    early_access_until: Option<DateTime<Utc>>,
    published_at: Option<DateTime<Utc>>,
}

/// Chapters readable for free right now. Premium chapters, and early
/// access ones until their window ends, need an account.
pub fn chapters(
    html: &str,
    slug: &str,
    base: &Url,
    now: DateTime<Utc>,
) -> Result<Vec<ChapterInfo>> {
    let list: ChapterList = island(html, "ChapterListReact")?;
    let mut out = Vec::new();
    for c in list.chapters {
        if c.is_premium || c.early_access_until.is_some_and(|until| until > now) {
            continue;
        }
        let number = c.number.to_string();
        let Ok(parsed) = number.parse() else {
            continue;
        };
        out.push(ChapterInfo {
            key: format!("{slug}/{number}").into(),
            number: parsed,
            volume: None,
            title: c.title.filter(|t| !t.trim().is_empty()),
            url: base
                .join(&format!("/comics/{slug}/chapter/{number}"))
                .map_err(|e| SourceError::parse("chapter URL", e.to_string()))?,
            language: Some("en".into()),
            scanlator: None,
            published_at: c.published_at,
        });
    }
    Ok(out)
}

#[derive(Deserialize)]
struct Reader {
    pages: Vec<Page>,
}

#[derive(Deserialize)]
struct Page {
    url: Url,
}

pub fn pages(html: &str) -> Result<Vec<Url>> {
    let reader: Reader = island(html, "ChapterReader")?;
    if reader.pages.is_empty() {
        return Err(SourceError::parse(
            "chapter pages",
            "no pages (locked chapter?)",
        ));
    }
    Ok(reader.pages.into_iter().map(|p| p.url).collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn decodes_astro_props() {
        let raw = json!({
            "name": [0, "x"],
            "count": [0, 3],
            "missing": [0],
            "list": [1, [[0, { "id": [0, 1] }], [0, { "id": [0, 2] }]]]
        });
        assert_eq!(
            decode(&raw),
            json!({ "name": "x", "count": 3, "missing": null, "list": [{ "id": 1 }, { "id": 2 }] })
        );
    }

    #[test]
    fn strips_the_rotating_suffix() {
        assert_eq!(bare_slug("solo-leveling-bd5bdaf8"), "solo-leveling");
        assert_eq!(bare_slug("solo-leveling"), "solo-leveling");
        assert_eq!(bare_slug("tomb-raider-kin-g-bd5bdaf8"), "tomb-raider-kin-g");
        assert_eq!(
            bare_slug("the-greatest-estate-developer"),
            "the-greatest-estate-developer"
        );
    }
}
