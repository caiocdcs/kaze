/// Chapter naming scheme parser and formatter.
///
/// Placeholders:
///   %M = Manga name
///   %V = Volume number
///   %C = Chapter number
///   %T = Chapter title
///   %A = First author name
///   %Y = Year
///
/// Conditional: ?X(...) - include content inside parens only if X is non-null.
/// Example: "?V(Vol.%V )" -> "Vol.1 " if volume=1, "" if volume=null.
use crate::utils::clean_name_for_windows;

pub struct ChapterNamingContext {
    pub manga_name: String,
    pub volume_number: Option<i32>,
    pub chapter_number: String,
    pub chapter_title: Option<String>,
    pub author_name: Option<String>,
    pub year: Option<u32>,
    pub always_include_volume: bool,
}

/// Resolves a naming scheme template to a filename (without extension).
pub fn resolve_naming_scheme(scheme: &str, ctx: &ChapterNamingContext) -> String {
    let resolved = resolve_conditionals(scheme, ctx);
    let replaced = resolve_placeholders(&resolved, ctx);
    let cleaned = clean_name_for_windows(&replaced);
    format!("{cleaned}.cbz")
}

/// Resolves conditional blocks: ?X(content) -> content if X is non-null, else "".
fn resolve_conditionals(scheme: &str, ctx: &ChapterNamingContext) -> String {
    let mut result = String::new();
    let chars: Vec<char> = scheme.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '?' && i + 1 < chars.len() {
            let placeholder_char = chars[i + 1];
            i += 2;

            if i < chars.len() && chars[i] == '(' {
                i += 1;
                let mut depth = 1;
                let mut content = String::new();
                while i < chars.len() && depth > 0 {
                    if chars[i] == '(' {
                        depth += 1;
                    } else if chars[i] == ')' {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    content.push(chars[i]);
                    i += 1;
                }

                let is_null = is_placeholder_null(placeholder_char, ctx);
                if !is_null {
                    result.push_str(&content);
                }
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Replaces %X placeholders with their values.
fn resolve_placeholders(scheme: &str, ctx: &ChapterNamingContext) -> String {
    let mut result = String::new();
    let chars: Vec<char> = scheme.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '%' && i + 1 < chars.len() {
            let value = placeholder_value(chars[i + 1], ctx);
            result.push_str(&value);
            i += 2;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

fn is_placeholder_null(c: char, ctx: &ChapterNamingContext) -> bool {
    match c {
        'M' => ctx.manga_name.is_empty(),
        'V' => {
            if ctx.always_include_volume {
                false
            } else {
                ctx.volume_number.is_none()
            }
        }
        'C' => ctx.chapter_number.is_empty(),
        'T' => ctx.chapter_title.is_none(),
        'A' => ctx.author_name.is_none(),
        'Y' => ctx.year.is_none(),
        _ => true,
    }
}

fn placeholder_value(c: char, ctx: &ChapterNamingContext) -> String {
    match c {
        'M' => ctx.manga_name.clone(),
        'V' => {
            if ctx.always_include_volume && ctx.volume_number.is_none() {
                "0".into()
            } else {
                ctx.volume_number.map(|v| v.to_string()).unwrap_or_default()
            }
        }
        'C' => ctx.chapter_number.clone(),
        'T' => ctx.chapter_title.clone().unwrap_or_default(),
        'A' => ctx.author_name.clone().unwrap_or_default(),
        'Y' => ctx.year.map(|y| y.to_string()).unwrap_or_default(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_ctx() -> ChapterNamingContext {
        ChapterNamingContext {
            manga_name: "One Piece".into(),
            volume_number: Some(1),
            chapter_number: "42".into(),
            chapter_title: Some("The Pirate King".into()),
            author_name: Some("Oda".into()),
            year: Some(1997),
            always_include_volume: false,
        }
    }

    #[test]
    fn default_scheme_resolves() {
        let ctx = default_ctx();
        let result = resolve_naming_scheme("%M - ?V(Vol.%V )Ch.%C?T( - %T)", &ctx);
        assert_eq!(result, "One Piece - Vol.1 Ch.42 - The Pirate King.cbz");
    }

    #[test]
    fn no_title_skips_conditional() {
        let mut ctx = default_ctx();
        ctx.chapter_title = None;
        let result = resolve_naming_scheme("%M - ?V(Vol.%V )Ch.%C?T( - %T)", &ctx);
        assert_eq!(result, "One Piece - Vol.1 Ch.42.cbz");
    }

    #[test]
    fn no_volume_skips_conditional() {
        let mut ctx = default_ctx();
        ctx.volume_number = None;
        let result = resolve_naming_scheme("%M - ?V(Vol.%V )Ch.%C", &ctx);
        assert_eq!(result, "One Piece - Ch.42.cbz");
    }

    #[test]
    fn always_include_volume_shows_zero() {
        let mut ctx = default_ctx();
        ctx.volume_number = None;
        ctx.always_include_volume = true;
        let result = resolve_naming_scheme("%M - ?V(Vol.%V )Ch.%C", &ctx);
        assert_eq!(result, "One Piece - Vol.0 Ch.42.cbz");
    }

    #[test]
    fn simple_scheme() {
        let ctx = default_ctx();
        let result = resolve_naming_scheme("%M Ch.%C", &ctx);
        assert_eq!(result, "One Piece Ch.42.cbz");
    }

    #[test]
    fn author_placeholder() {
        let ctx = default_ctx();
        let result = resolve_naming_scheme("%A - %M Ch.%C", &ctx);
        assert_eq!(result, "Oda - One Piece Ch.42.cbz");
    }

    #[test]
    fn year_placeholder() {
        let ctx = default_ctx();
        let result = resolve_naming_scheme("%M (%Y) Ch.%C", &ctx);
        assert_eq!(result, "One Piece (1997) Ch.42.cbz");
    }

    #[test]
    fn null_year_skips_conditional() {
        let mut ctx = default_ctx();
        ctx.year = None;
        let result = resolve_naming_scheme("%M?Y( (%Y)) Ch.%C", &ctx);
        assert_eq!(result, "One Piece Ch.42.cbz");
    }

    #[test]
    fn forbidden_chars_cleaned() {
        let mut ctx = default_ctx();
        ctx.manga_name = "Manga: The Series?".into();
        let result = resolve_naming_scheme("%M Ch.%C", &ctx);
        assert_eq!(result, "Manga The Series Ch.42.cbz");
    }

    #[test]
    fn nested_parens_in_conditional() {
        let ctx = default_ctx();
        let result = resolve_naming_scheme("?V(Vol.%V )Ch.%C", &ctx);
        assert_eq!(result, "Vol.1 Ch.42.cbz");
    }
}
