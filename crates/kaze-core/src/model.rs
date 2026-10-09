use std::fmt;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::ChapterNumber;

/// Declares a newtype around `String` for ids that only mean something on
/// one source. Distinct types stop us from passing a chapter key where a
/// series key is expected.
macro_rules! string_key {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
    };
}

string_key!(
    /// Id of a series on its source, e.g. WeebCentral's series ULID.
    SeriesKey
);
string_key!(
    /// Id of a chapter on its source.
    ChapterKey
);

/// A search result: just enough to show a card and pick it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesHit {
    pub key: SeriesKey,
    pub title: String,
    pub url: Url,
    pub cover_url: Option<Url>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeriesStatus {
    Ongoing,
    Completed,
    Hiatus,
    Cancelled,
    #[default]
    Unknown,
}

/// Full metadata of a series as the source describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    pub key: SeriesKey,
    pub title: String,
    pub alt_titles: Vec<String>,
    pub url: Url,
    pub cover_url: Option<Url>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
    pub status: SeriesStatus,
    pub year: Option<u16>,
    /// ISO 639-1 code of the language the chapters are in, when known.
    pub language: Option<String>,
}

/// One chapter as listed by the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChapterInfo {
    pub key: ChapterKey,
    pub number: ChapterNumber,
    pub volume: Option<u32>,
    pub title: Option<String>,
    pub url: Url,
    pub language: Option<String>,
    pub scanlator: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
}

/// A downloaded page image, exactly as the server sent it.
#[derive(Debug, Clone)]
pub struct Image {
    pub bytes: Bytes,
    /// The `Content-Type` header; often wrong (WeebCentral serves JPEGs as
    /// `.png`), so writers should sniff `bytes` before trusting it.
    pub content_type: Option<String>,
}
