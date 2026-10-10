//! Small helpers for scraping HTML with `scraper`, shared by sources.

use scraper::ElementRef;

/// A CSS selector compiled once, on first use, and reused afterwards.
///
/// Each call site gets its own `static`, so `sel!("a.title")` in a loop
/// costs a pointer read, not a parse.
macro_rules! sel {
    ($css:literal) => {{
        #[allow(clippy::expect_used, reason = "literal selector, covered by tests")]
        static SELECTOR: ::std::sync::LazyLock<::scraper::Selector> =
            ::std::sync::LazyLock::new(|| {
                ::scraper::Selector::parse($css).expect(concat!("invalid selector: ", $css))
            });
        &*SELECTOR
    }};
}
pub(crate) use sel;

/// Text content of an element with whitespace collapsed, `None` if empty.
pub fn text(element: ElementRef<'_>) -> Option<String> {
    let joined = element.text().collect::<Vec<_>>().join(" ");
    let collapsed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    (!collapsed.is_empty()).then_some(collapsed)
}
