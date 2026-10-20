//! Language tags in BCP 47 casing: `en`, `pt-BR`, `zh-Hant`, `es-419`.
//! Sites spell them differently (MangaDex uses `pt-br`), so every tag is
//! normalized once and stored in one form.

/// `pt-br`, `PT_BR` and `pt-BR` all become `pt-BR`. `None` if `tag` is not
/// a language tag kaze understands: a 2–3 letter language, then optional
/// script (4 letters) and region (2 letters or 3 digits) subtags.
#[must_use]
pub fn normalize(tag: &str) -> Option<String> {
    let mut parts = tag.trim().split(['-', '_']);
    let language = parts.next()?;
    if !(2..=3).contains(&language.len()) || !language.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let mut out = language.to_ascii_lowercase();
    for part in parts {
        let alpha = part.bytes().all(|b| b.is_ascii_alphabetic());
        let subtag = match part.len() {
            2 if alpha => part.to_ascii_uppercase(),
            3 if part.bytes().all(|b| b.is_ascii_digit()) => part.to_owned(),
            4 if alpha => {
                let lower = part.to_ascii_lowercase();
                lower[..1].to_ascii_uppercase() + &lower[1..]
            }
            _ => return None,
        };
        out.push('-');
        out.push_str(&subtag);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_tags() {
        let cases = [
            ("en", "en"),
            ("EN", "en"),
            ("pt-br", "pt-BR"),
            ("PT_BR", "pt-BR"),
            (" pt-BR ", "pt-BR"),
            ("zh-hant", "zh-Hant"),
            ("es-419", "es-419"),
        ];
        for (tag, want) in cases {
            assert_eq!(normalize(tag).as_deref(), Some(want), "{tag}");
        }
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "e", "english", "pt-", "pt-b", "1a", "pt-br-x"] {
            assert_eq!(normalize(bad), None, "{bad}");
        }
    }
}
