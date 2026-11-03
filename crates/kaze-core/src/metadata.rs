//! Ids of a series in metadata databases, apart from the sites chapters
//! come from.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A manga's id on [AniList](https://anilist.co).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AniListId(pub u32);

/// Where a series is listed in metadata databases.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExternalIds {
    pub anilist: Option<AniListId>,
}

impl fmt::Display for AniListId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
