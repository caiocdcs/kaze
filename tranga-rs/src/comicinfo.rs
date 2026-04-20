/// Generates ComicInfo.xml content for a chapter.
/// Follows the ComicInfo standard used by Komga/Kavita.
pub struct ComicInfoData {
    pub chapter_number: String,
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub volume_number: Option<i32>,
    pub authors: Vec<String>,
    pub original_language: Option<String>,
    pub description: String,
}

pub fn generate_comic_info_xml(data: &ComicInfoData) -> String {
    let mut elements = Vec::new();

    elements.push(format!(
        "<Number>{}</Number>",
        xml_escape(&data.chapter_number)
    ));

    if let Some(ref title) = data.title {
        elements.push(format!("<Title>{}</Title>", xml_escape(title)));
    }

    if !data.tags.is_empty() {
        elements.push(format!("<Tags>{}</Tags>", xml_escape(&data.tags.join(","))));
    }

    if let Some(vol) = data.volume_number {
        elements.push(format!("<Volume>{vol}</Volume>"));
    }

    if !data.authors.is_empty() {
        elements.push(format!(
            "<Writer>{}</Writer>",
            xml_escape(&data.authors.join(","))
        ));
    }

    if let Some(ref lang) = data.original_language {
        elements.push(format!("<LanguageISO>{}</LanguageISO>", xml_escape(lang)));
    }

    if !data.description.is_empty() {
        elements.push(format!(
            "<Summary>{}</Summary>",
            xml_escape(&data.description)
        ));
    }

    format!("<ComicInfo>\n  {}\n</ComicInfo>", elements.join("\n  "))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_comic_info() {
        let data = ComicInfoData {
            chapter_number: "1".into(),
            title: None,
            tags: vec![],
            volume_number: None,
            authors: vec![],
            original_language: None,
            description: String::new(),
        };
        let xml = generate_comic_info_xml(&data);
        assert!(xml.contains("<Number>1</Number>"));
        assert!(xml.starts_with("<ComicInfo>"));
        assert!(xml.ends_with("</ComicInfo>"));
        assert!(!xml.contains("<Title>"));
        assert!(!xml.contains("<Tags>"));
        assert!(!xml.contains("<Volume>"));
    }

    #[test]
    fn full_comic_info() {
        let data = ComicInfoData {
            chapter_number: "42".into(),
            title: Some("The Pirate King".into()),
            tags: vec!["action".into(), "adventure".into()],
            volume_number: Some(1),
            authors: vec!["Oda".into()],
            original_language: Some("ja".into()),
            description: "A pirate manga".into(),
        };
        let xml = generate_comic_info_xml(&data);
        assert!(xml.contains("<Number>42</Number>"));
        assert!(xml.contains("<Title>The Pirate King</Title>"));
        assert!(xml.contains("<Tags>action,adventure</Tags>"));
        assert!(xml.contains("<Volume>1</Volume>"));
        assert!(xml.contains("<Writer>Oda</Writer>"));
        assert!(xml.contains("<LanguageISO>ja</LanguageISO>"));
        assert!(xml.contains("<Summary>A pirate manga</Summary>"));
    }

    #[test]
    fn xml_escapes_special_chars() {
        let data = ComicInfoData {
            chapter_number: "1".into(),
            title: Some("Tom & Jerry <Special> \"Episode\"".into()),
            tags: vec![],
            volume_number: None,
            authors: vec![],
            original_language: None,
            description: String::new(),
        };
        let xml = generate_comic_info_xml(&data);
        assert!(xml.contains("&amp;"), "should escape &");
        assert!(xml.contains("&lt;"), "should escape <");
        assert!(xml.contains("&gt;"), "should escape >");
        assert!(xml.contains("&quot;"), "should escape \"");
        assert!(!xml.contains("<Special>"), "should not have raw <");
    }

    #[test]
    fn multiple_authors_joined() {
        let data = ComicInfoData {
            chapter_number: "1".into(),
            title: None,
            tags: vec![],
            volume_number: None,
            authors: vec!["Author A".into(), "Author B".into()],
            original_language: None,
            description: String::new(),
        };
        let xml = generate_comic_info_xml(&data);
        assert!(xml.contains("<Writer>Author A,Author B</Writer>"));
    }

    #[test]
    fn empty_description_not_rendered() {
        let data = ComicInfoData {
            chapter_number: "1".into(),
            title: None,
            tags: vec![],
            volume_number: None,
            authors: vec![],
            original_language: None,
            description: String::new(),
        };
        let xml = generate_comic_info_xml(&data);
        assert!(!xml.contains("<Summary>"));
    }
}
