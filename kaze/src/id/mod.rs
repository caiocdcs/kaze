pub mod alt_title_id;
pub mod author_id;
pub mod chapter_id;
pub mod connector_mapping_id;
pub mod library_id;
pub mod link_id;
pub mod manga_id;

pub use alt_title_id::AltTitleId;
pub use author_id::AuthorId;
pub use chapter_id::ChapterId;
pub use connector_mapping_id::ConnectorMappingId;
pub use library_id::FileLibraryId;
pub use link_id::LinkId;
pub use manga_id::MangaId;

use ulid::Ulid;

pub trait EntityId:
    std::fmt::Debug
    + std::fmt::Display
    + Clone
    + PartialEq
    + Eq
    + std::hash::Hash
    + Send
    + Sync
    + 'static
{
    const PREFIX: &'static str;

    fn from_ulid(ulid: Ulid) -> Self;
    fn as_ulid(&self) -> &Ulid;

    fn new() -> Self
    where
        Self: Sized,
    {
        Self::from_ulid(Ulid::new())
    }

    fn from_strict(s: &str) -> Result<Self, IdParseError>
    where
        Self: Sized,
    {
        let (prefix, ulid_str) = s.split_once('_').ok_or(IdParseError::MissingSeparator)?;
        if prefix != Self::PREFIX {
            return Err(IdParseError::WrongPrefix {
                expected: Self::PREFIX,
                got: prefix.to_string(),
            });
        }
        let ulid =
            Ulid::from_string(ulid_str).map_err(|e| IdParseError::InvalidUlid(e.to_string()))?;
        Ok(Self::from_ulid(ulid))
    }

    fn to_prefixed(&self) -> String {
        format!("{}_{}", Self::PREFIX, self.as_ulid())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IdParseError {
    #[error("id missing '_' separator")]
    MissingSeparator,
    #[error("wrong prefix: expected {expected}, got {got}")]
    WrongPrefix { expected: &'static str, got: String },
    #[error("invalid ulid: {0}")]
    InvalidUlid(String),
}

macro_rules! define_id_type {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(ulid::Ulid);

        impl $crate::id::EntityId for $name {
            const PREFIX: &'static str = $prefix;
            fn from_ulid(ulid: ulid::Ulid) -> Self {
                Self(ulid)
            }
            fn as_ulid(&self) -> &ulid::Ulid {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}_{}", $prefix, self.0)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&format!("{}_{}", $prefix, self.0))
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                <$name as $crate::id::EntityId>::from_strict(&s).map_err(serde::de::Error::custom)
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::id::IdParseError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::from_strict(s)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

pub(crate) use define_id_type;

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    define_id_type!(TestId, "test");

    #[test]
    fn new_id_has_correct_prefix() {
        let id = TestId::new();
        let displayed = id.to_string();
        assert!(
            displayed.starts_with("test_"),
            "id should start with 'test_', got: {displayed}"
        );
    }

    #[test]
    fn from_strict_roundtrip() {
        let id = TestId::new();
        let s = id.to_string();
        let parsed = TestId::from_strict(&s).expect("parse should succeed");
        assert_eq!(id, parsed, "roundtrip should produce equal id");
    }

    #[test]
    fn from_strict_rejects_missing_separator() {
        let result = TestId::from_strict("testNOSEP");
        assert!(matches!(result, Err(IdParseError::MissingSeparator)));
    }

    #[test]
    fn from_strict_rejects_wrong_prefix() {
        let manga_id = MangaId::new().to_string();
        let result = TestId::from_strict(&manga_id);
        assert!(matches!(result, Err(IdParseError::WrongPrefix { .. })));
    }

    #[test]
    fn from_strict_rejects_invalid_ulid() {
        let result = TestId::from_strict("test_XXXXX");
        assert!(matches!(result, Err(IdParseError::InvalidUlid(_))));
    }

    #[test]
    fn different_ids_are_not_equal() {
        let a = TestId::new();
        let b = TestId::new();
        assert_ne!(a, b, "two new ids should be distinct");
    }

    #[test]
    fn from_str_trait_works() {
        let id = TestId::new();
        let s = id.to_string();
        let parsed: TestId = s.parse().expect("FromStr should work");
        assert_eq!(id, parsed);
    }

    #[test]
    fn serde_roundtrip() {
        let id = TestId::new();
        let json = serde_json::to_string(&id).expect("serialize");
        let parsed: TestId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(id, parsed, "serde roundtrip should produce equal id");
    }

    #[test]
    fn serde_rejects_bad_prefix() {
        let json = r#""chapter_01ARZ3NDEKTSV4RRFFQ69G5FAKV""#;
        let result: Result<MangaId, _> = serde_json::from_str(json);
        assert!(result.is_err(), "deserializing wrong prefix should fail");
    }

    #[test]
    fn default_creates_new_id() {
        let a = TestId::default();
        let b = TestId::default();
        assert_ne!(a, b);
        assert!(a.to_string().starts_with("test_"));
    }

    #[test]
    fn ulid_extraction_works() {
        let id = TestId::new();
        let ulid = id.as_ulid();
        assert_eq!(
            format!("{ulid}").len(),
            26,
            "ulid string should be 26 chars"
        );
    }

    #[test]
    fn ids_are_usable_as_hash_keys() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        let a = TestId::new();
        let b = TestId::new();
        set.insert(a.clone());
        set.insert(b.clone());
        assert_eq!(set.len(), 2);
        assert!(set.contains(&a));
        assert!(set.contains(&b));
    }
}
