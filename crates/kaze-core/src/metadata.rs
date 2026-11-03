//! Ids of a series in metadata databases, apart from the sites chapters
//! come from.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A manga's id on [AniList](https://anilist.co).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AniListId(pub u32);

impl fmt::Display for AniListId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
