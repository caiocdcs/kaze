/// Strips characters forbidden in Windows filenames.
/// Removes: " * / : < > ? \ | and control chars (< 32).
pub fn clean_name_for_windows(name: &str) -> String {
    name.chars()
        .filter(|c| {
            !matches!(
                c,
                '"' | '*' | '/' | ':' | '<' | '>' | '?' | '\\' | '|' | '\x00'..='\x1F'
            )
        })
        .collect()
}

/// Sanitizes text for safe JSON serialization.
/// Replaces control characters (U+0000-U+001F) with spaces, except common whitespace.
pub fn sanitize_for_json(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                c
            } else if c.is_ascii_control() {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Builds a URI from a base URL and relative path, ensuring no double slashes.
pub fn build_uri(base_url: &str, relative_path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let rel = relative_path.trim_start_matches('/');
    format!("{base}/{rel}")
}

/// Strips leading zeros from numeric chapter number parts.
/// "01" -> "1", "1.5" -> "1.5", "001.010" -> "1.10"
pub fn normalize_chapter_number(chapter: &str) -> String {
    chapter
        .split('.')
        .map(|part| {
            if part.is_empty() {
                return part.to_string();
            }
            match part.parse::<i64>() {
                Ok(n) => n.to_string(),
                Err(_) => part.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_name_strips_forbidden_chars() {
        assert_eq!(clean_name_for_windows("hello:world"), "helloworld");
        assert_eq!(clean_name_for_windows("file?1<2>3"), "file123");
        assert_eq!(clean_name_for_windows(r#"a\b/c*d"#), "abcd");
        assert_eq!(clean_name_for_windows("x|y\"z"), "xyz");
    }

    #[test]
    fn clean_name_strips_control_chars() {
        assert_eq!(clean_name_for_windows("hello\x00world"), "helloworld");
        assert_eq!(clean_name_for_windows("tab\there"), "tabhere");
        assert_eq!(clean_name_for_windows("newline\nhere"), "newlinehere");
    }

    #[test]
    fn clean_name_preserves_valid_chars() {
        assert_eq!(
            clean_name_for_windows("Vol.1 Ch.2 - Title (JP)"),
            "Vol.1 Ch.2 - Title (JP)"
        );
        assert_eq!(clean_name_for_windows("Manga Name!"), "Manga Name!");
    }

    #[test]
    fn build_uri_no_double_slash() {
        assert_eq!(
            build_uri("http://example.com/", "/api/test"),
            "http://example.com/api/test"
        );
        assert_eq!(
            build_uri("http://example.com", "api/test"),
            "http://example.com/api/test"
        );
        assert_eq!(
            build_uri("http://example.com/", "api/test"),
            "http://example.com/api/test"
        );
    }

    #[test]
    fn normalize_chapter_strips_leading_zeros() {
        assert_eq!(normalize_chapter_number("01"), "1");
        assert_eq!(normalize_chapter_number("001"), "1");
        assert_eq!(normalize_chapter_number("1.5"), "1.5");
        assert_eq!(normalize_chapter_number("001.010"), "1.10");
    }

    #[test]
    fn normalize_chapter_preserves_non_numeric() {
        assert_eq!(normalize_chapter_number("1a"), "1a");
        assert_eq!(normalize_chapter_number("extra"), "extra");
    }

    #[test]
    fn normalize_chapter_single_digit() {
        assert_eq!(normalize_chapter_number("5"), "5");
        assert_eq!(normalize_chapter_number("0"), "0");
    }

    #[test]
    fn normalize_chapter_multi_part() {
        assert_eq!(normalize_chapter_number("10.5.2"), "10.5.2");
    }

    #[test]
    fn sanitize_for_json_preserves_normal_text() {
        assert_eq!(sanitize_for_json("hello world"), "hello world");
        assert_eq!(sanitize_for_json("line1\nline2"), "line1\nline2");
        assert_eq!(sanitize_for_json("tab\there"), "tab\there");
    }

    #[test]
    fn sanitize_for_json_replaces_control_chars() {
        assert_eq!(sanitize_for_json("hello\x00world"), "hello world");
        assert_eq!(sanitize_for_json("test\x1Fend"), "test end");
    }

    #[test]
    fn sanitize_for_json_preserves_unicode() {
        assert_eq!(sanitize_for_json("日本語"), "日本語");
        assert_eq!(sanitize_for_json("emoji 🎉"), "emoji 🎉");
    }
}
