//! Chapter file names from a template, e.g. `%M - ?V(Vol.%V )Ch.%C?T( - %T)`.
//!
//! | Code | Value            |
//! |------|------------------|
//! | `%M` | series title     |
//! | `%V` | volume           |
//! | `%C` | chapter number   |
//! | `%T` | chapter title    |
//! | `%A` | first author     |
//! | `%Y` | year             |
//! | `%%` | a literal `%`    |
//!
//! `?X(...)` keeps its content only when field `X` has a value, so
//! `?V(Vol.%V )` disappears for chapters without a volume.
//!
//! The template is parsed once into a small syntax tree: a bad template is
//! rejected when it is saved instead of producing odd file names later.

use std::{fmt, iter::Peekable, str::Chars, str::FromStr};

use super::ChapterMeta;

pub const DEFAULT_TEMPLATE: &str = "%M - ?V(Vol.%V )Ch.%C?T( - %T)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Series,
    Volume,
    Chapter,
    Title,
    Author,
    Year,
}

impl Field {
    fn from_code(code: char) -> Option<Self> {
        Some(match code {
            'M' => Self::Series,
            'V' => Self::Volume,
            'C' => Self::Chapter,
            'T' => Self::Title,
            'A' => Self::Author,
            'Y' => Self::Year,
            _ => return None,
        })
    }

    fn value(self, meta: &ChapterMeta) -> Option<String> {
        match self {
            Self::Series => Some(meta.series_title.clone()),
            Self::Volume => meta.volume.map(|v| v.to_string()),
            Self::Chapter => Some(meta.number.to_string()),
            Self::Title => meta.title.clone(),
            Self::Author => meta.authors.first().cloned(),
            Self::Year => meta.year.map(|y| y.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Text(String),
    Field(Field),
    /// `?X(...)`: rendered only when `X` has a value.
    IfSome(Field, Vec<Segment>),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    #[error("unknown placeholder %{0}")]
    UnknownField(char),
    #[error("`%` at the end of the template")]
    DanglingPercent,
    #[error("`?{0}` must be followed by `(`")]
    MissingParen(char),
    #[error("unclosed `(`")]
    Unclosed,
    #[error("unexpected `)`")]
    UnexpectedClose,
    #[error("the template must contain %C so chapters get distinct names")]
    NoChapter,
}

/// A parsed file name template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    source: String,
    segments: Vec<Segment>,
}

impl Template {
    /// The file name (without extension) for a chapter, safe for any OS.
    #[must_use]
    pub fn render(&self, meta: &ChapterMeta) -> String {
        let mut out = String::new();
        render_into(&self.segments, meta, &mut out);
        sanitize_file_name(&out)
    }
}

impl Default for Template {
    fn default() -> Self {
        DEFAULT_TEMPLATE.parse().expect("default template is valid")
    }
}

impl fmt::Display for Template {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl FromStr for Template {
    type Err = TemplateError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let mut chars = source.chars().peekable();
        let segments = parse_until_close(&mut chars, false)?;
        if !contains_chapter(&segments) {
            return Err(TemplateError::NoChapter);
        }
        Ok(Self {
            source: source.to_owned(),
            segments,
        })
    }
}

/// Recursive descent: each `?X(` parses its content with a nested call
/// that stops at the matching `)`.
fn parse_until_close(
    chars: &mut Peekable<Chars<'_>>,
    nested: bool,
) -> Result<Vec<Segment>, TemplateError> {
    let mut segments = Vec::new();
    let mut text = String::new();
    let flush = |text: &mut String, segments: &mut Vec<Segment>| {
        if !text.is_empty() {
            segments.push(Segment::Text(std::mem::take(text)));
        }
    };

    while let Some(c) = chars.next() {
        match c {
            '%' => match chars.next() {
                Some('%') => text.push('%'),
                Some(code) => {
                    let field = Field::from_code(code).ok_or(TemplateError::UnknownField(code))?;
                    flush(&mut text, &mut segments);
                    segments.push(Segment::Field(field));
                }
                None => return Err(TemplateError::DanglingPercent),
            },
            // A `?` not followed by a field code is just text ("Why?").
            '?' if chars.peek().copied().and_then(Field::from_code).is_some() => {
                let code = chars.next().expect("peeked");
                if chars.next() != Some('(') {
                    return Err(TemplateError::MissingParen(code));
                }
                let field = Field::from_code(code).expect("checked above");
                let inner = parse_until_close(chars, true)?;
                flush(&mut text, &mut segments);
                segments.push(Segment::IfSome(field, inner));
            }
            ')' if nested => {
                flush(&mut text, &mut segments);
                return Ok(segments);
            }
            ')' => return Err(TemplateError::UnexpectedClose),
            c => text.push(c),
        }
    }
    if nested {
        return Err(TemplateError::Unclosed);
    }
    flush(&mut text, &mut segments);
    Ok(segments)
}

fn contains_chapter(segments: &[Segment]) -> bool {
    // Only an unconditional %C guarantees distinct names.
    segments
        .iter()
        .any(|s| matches!(s, Segment::Field(Field::Chapter)))
}

fn render_into(segments: &[Segment], meta: &ChapterMeta, out: &mut String) {
    for segment in segments {
        match segment {
            Segment::Text(text) => out.push_str(text),
            Segment::Field(field) => out.push_str(&field.value(meta).unwrap_or_default()),
            Segment::IfSome(field, inner) => {
                if field.value(meta).is_some() {
                    render_into(inner, meta, out);
                }
            }
        }
    }
}

/// Make a string safe as a file or folder name on Windows, macOS and Linux.
///
/// Drops characters Windows forbids and control characters, collapses
/// whitespace, trims trailing dots/spaces (invalid on Windows) and caps
/// the length well under common 255-byte limits. Other Unicode is kept.
#[must_use]
pub fn sanitize_file_name(name: &str) -> String {
    const MAX_BYTES: usize = 200;
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            ':' => '-',
            '/' | '\\' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .filter(|c| !matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
        .collect();
    let mut collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() > MAX_BYTES {
        let mut end = MAX_BYTES;
        while !collapsed.is_char_boundary(end) {
            end -= 1;
        }
        collapsed.truncate(end);
    }
    let trimmed = collapsed.trim_end_matches(['.', ' ']).trim_start();
    if trimmed.is_empty() {
        "_".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> ChapterMeta {
        ChapterMeta {
            series_title: "One Piece".into(),
            number: "1095.5".parse().unwrap(),
            volume: None,
            title: None,
            ..ChapterMeta::default()
        }
    }

    #[test]
    fn default_template() {
        let t = Template::default();
        assert_eq!(t.render(&meta()), "One Piece - Ch.1095.5");

        let full = ChapterMeta {
            volume: Some(3),
            title: Some("Romance Dawn".into()),
            ..meta()
        };
        assert_eq!(
            t.render(&full),
            "One Piece - Vol.3 Ch.1095.5 - Romance Dawn"
        );
    }

    #[test]
    fn nested_conditionals_and_literals() {
        let t: Template = "[%Y] %C?T(?A(%A: )%T) 100%%?".parse().unwrap();
        let m = ChapterMeta {
            year: Some(1997),
            authors: vec!["Oda".into()],
            title: Some("Start".into()),
            ..meta()
        };
        // `?` at the end is plain text, then stripped as a forbidden char.
        assert_eq!(t.render(&m), "[1997] 1095.5Oda- Start 100%");
        assert_eq!(t.render(&meta()), "[] 1095.5 100%");
    }

    #[test]
    fn rejects_bad_templates() {
        assert_eq!(
            "%M %X".parse::<Template>(),
            Err(TemplateError::UnknownField('X'))
        );
        assert_eq!(
            "%C %".parse::<Template>(),
            Err(TemplateError::DanglingPercent)
        );
        assert_eq!(
            "%C ?V(Vol".parse::<Template>(),
            Err(TemplateError::Unclosed)
        );
        assert_eq!(
            "%C ?V Vol".parse::<Template>(),
            Err(TemplateError::MissingParen('V'))
        );
        assert_eq!(
            "%C)".parse::<Template>(),
            Err(TemplateError::UnexpectedClose)
        );
        assert_eq!(
            "%M ?C(%C)".parse::<Template>(),
            Err(TemplateError::NoChapter)
        );
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_file_name("Re:Zero / Part 2?"), "Re-Zero Part 2");
        assert_eq!(sanitize_file_name("  Dr. Stone...  "), "Dr. Stone");
        assert_eq!(sanitize_file_name("進撃の巨人"), "進撃の巨人");
        assert_eq!(sanitize_file_name("???"), "_");
        let long = "あ".repeat(100); // 300 bytes
        let cut = sanitize_file_name(&long);
        assert!(cut.len() <= 200 && cut.chars().all(|c| c == 'あ'));
    }
}
