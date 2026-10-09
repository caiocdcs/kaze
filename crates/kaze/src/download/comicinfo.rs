//! `ComicInfo.xml`, the metadata file Komga, Kavita and most readers look
//! for inside a CBZ. Schema: <https://anansi-project.github.io/docs/comicinfo/intro>.

use std::fmt::Write as _;

use super::ChapterMeta;

#[must_use]
pub fn render(meta: &ChapterMeta, page_count: usize) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n",
    );
    let mut field = |name: &str, value: &str| {
        if !value.is_empty() {
            let _ = writeln!(xml, "  <{name}>{}</{name}>", escape(value));
        }
    };

    field("Title", meta.title.as_deref().unwrap_or_default());
    field("Series", &meta.series_title);
    field("Number", &meta.number.to_string());
    field(
        "Volume",
        &meta.volume.map(|v| v.to_string()).unwrap_or_default(),
    );
    field("Summary", meta.summary.as_deref().unwrap_or_default());
    field(
        "Year",
        &meta.year.map(|y| y.to_string()).unwrap_or_default(),
    );
    field("Writer", &meta.authors.join(", "));
    field("Genre", &meta.tags.join(", "));
    field("Web", meta.url.as_deref().unwrap_or_default());
    field("PageCount", &page_count.to_string());
    field("LanguageISO", meta.language.as_deref().unwrap_or_default());
    field("Manga", "Yes");

    xml.push_str("</ComicInfo>\n");
    xml
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters other than tab/newline are invalid in XML 1.0.
            c if c.is_control() && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_and_escapes() {
        let meta = ChapterMeta {
            series_title: "Tom & Jerry <3".into(),
            number: "12.5".parse().unwrap(),
            volume: Some(2),
            title: None,
            authors: vec!["A".into(), "B".into()],
            tags: vec!["Action".into()],
            year: Some(2001),
            summary: Some("He said \"hi\"\u{0}".into()),
            language: Some("en".into()),
            url: Some("https://example.com/c?a=1&b=2".into()),
        };
        let xml = render(&meta, 16);
        assert!(xml.contains("<Series>Tom &amp; Jerry &lt;3</Series>"));
        assert!(xml.contains("<Number>12.5</Number>"));
        assert!(xml.contains("<Volume>2</Volume>"));
        assert!(xml.contains("<Writer>A, B</Writer>"));
        assert!(xml.contains("<Summary>He said &quot;hi&quot;</Summary>"));
        assert!(xml.contains("<Web>https://example.com/c?a=1&amp;b=2</Web>"));
        assert!(xml.contains("<PageCount>16</PageCount>"));
        assert!(!xml.contains("<Title>"), "empty fields are omitted");
    }
}
