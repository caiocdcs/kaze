use serde::{Deserialize, Deserializer};

/// One field of a partial (PATCH) update.
///
/// JSON has three cases for a nullable field: missing (leave it), `null`
/// (clear it) and a value (set it). `Option` alone cannot tell the first
/// two apart. With `#[serde(default)]` a missing field becomes `Keep`;
/// otherwise the impl below sees either `null` or a value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Patch<T> {
    #[default]
    Keep,
    Clear,
    Set(T),
}

impl<T> Patch<T> {
    pub fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }

    /// The new value of the field, given its current value.
    pub fn apply(self, current: Option<T>) -> Option<T> {
        match self {
            Self::Keep => current,
            Self::Clear => None,
            Self::Set(value) => Some(value),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Patch<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Self::Set(value),
            None => Self::Clear,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Body {
        #[serde(default)]
        field: Patch<u32>,
    }

    fn parse(json: &str) -> Patch<u32> {
        serde_json::from_str::<Body>(json).unwrap().field
    }

    #[test]
    fn three_states() {
        assert_eq!(parse("{}"), Patch::Keep);
        assert_eq!(parse(r#"{"field": null}"#), Patch::Clear);
        assert_eq!(parse(r#"{"field": 3}"#), Patch::Set(3));
        assert_eq!(Patch::Keep.apply(Some(1)), Some(1));
        assert_eq!(Patch::<u32>::Clear.apply(Some(1)), None);
    }
}
