use crate::id::EntityId;
use ulid::Ulid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AltTitleId(Ulid);

impl crate::id::EntityId for AltTitleId {
    const PREFIX: &'static str = "alt_title";
    fn from_ulid(ulid: Ulid) -> Self {
        Self(ulid)
    }
    fn as_ulid(&self) -> &Ulid {
        &self.0
    }
}

impl std::fmt::Display for AltTitleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "alt_title_{}", self.0)
    }
}

impl serde::Serialize for AltTitleId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("alt_title_{}", self.0))
    }
}

impl<'de> serde::Deserialize<'de> for AltTitleId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        crate::id::EntityId::from_strict(&s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for AltTitleId {
    type Err = crate::id::IdParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_strict(s)
    }
}

impl Default for AltTitleId {
    fn default() -> Self {
        Self::new()
    }
}
