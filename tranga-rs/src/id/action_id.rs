use crate::id::EntityId;
use ulid::Ulid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionId(Ulid);

impl crate::id::EntityId for ActionId {
    const PREFIX: &'static str = "action";
    fn from_ulid(ulid: Ulid) -> Self {
        Self(ulid)
    }
    fn as_ulid(&self) -> &Ulid {
        &self.0
    }
}

impl std::fmt::Display for ActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "action_{}", self.0)
    }
}

impl serde::Serialize for ActionId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("action_{}", self.0))
    }
}

impl<'de> serde::Deserialize<'de> for ActionId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        crate::id::EntityId::from_strict(&s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for ActionId {
    type Err = crate::id::IdParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_strict(s)
    }
}

impl Default for ActionId {
    fn default() -> Self {
        Self::new()
    }
}
