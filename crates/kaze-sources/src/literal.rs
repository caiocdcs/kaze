//! Values written in the source code, checked once on first use.
//!
//! A malformed literal is a bug in kaze, not a runtime condition, and the
//! fixture tests exercise every one of them.

/// A regex compiled once per call site.
macro_rules! re {
    ($pattern:literal) => {{
        #[allow(clippy::expect_used, reason = "literal pattern, covered by tests")]
        static RE: ::std::sync::LazyLock<::regex::Regex> = ::std::sync::LazyLock::new(|| {
            ::regex::Regex::new($pattern).expect(concat!("invalid regex: ", $pattern))
        });
        &*RE
    }};
}
pub(crate) use re;

/// A URL from a constant.
macro_rules! url {
    ($url:expr) => {{
        #[allow(clippy::expect_used, reason = "constant URL, covered by tests")]
        static URL: ::std::sync::LazyLock<::url::Url> =
            ::std::sync::LazyLock::new(|| ::url::Url::parse($url).expect("invalid URL constant"));
        URL.clone()
    }};
}
pub(crate) use url;
