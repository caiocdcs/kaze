use crate::id::EntityId;
use ulid::Ulid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LinkId(Ulid);

impl crate::id::EntityId for LinkId {
    const PREFIX: &'static str = "link";
    fn from_ulid(ulid: Ulid) -> Self {
        Self(ulid)
    }
    fn as_ulid(&self) -> &Ulid {
        &self.0
    }
}

impl std::fmt::Display for LinkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "link_{}", self.0)
    }
}

impl serde::Serialize for LinkId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("link_{}", self.0))
    }
}

impl<'de> serde::Deserialize<'de> for LinkId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        crate::id::EntityId::from_strict(&s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for LinkId {
    type Err = crate::id::IdParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_strict(s)
    }
}

impl Default for LinkId {
    fn default() -> Self {
        Self::new()
    }
}
