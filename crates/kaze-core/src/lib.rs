//! Domain types and the [`Source`] trait shared by kaze and its sources.
//!
//! This crate deliberately has no IO: it only describes *what* a manga
//! source looks like, so sources and the app can both depend on it.

mod chapter_number;
mod error;
mod model;
mod source;

pub use chapter_number::{ChapterNumber, ParseChapterNumberError};
pub use error::{Result, SourceError};
pub use model::{ChapterInfo, ChapterKey, Series, SeriesHit, SeriesKey, SeriesStatus};
pub use source::{RateLimit, Source, SourceMeta, UrlTarget};
