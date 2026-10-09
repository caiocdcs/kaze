use std::{borrow::Cow, cmp::Ordering, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// A chapter number such as `12`, `12.5` or `0`.
///
/// Stored as the whole part plus the fractional digits without trailing
/// zeros. That makes `12.50 == 12.5` and gives decimal ordering
/// (`12.15 < 12.5`) without floating-point surprises, so it can be `Eq`,
/// `Ord` and `Hash` and used as a map key.
/// The default is chapter `0`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ChapterNumber {
    whole: u32,
    frac: String,
}

impl ChapterNumber {
    #[must_use]
    pub const fn whole(&self) -> u32 {
        self.whole
    }

    #[must_use]
    pub fn is_whole(&self) -> bool {
        self.frac.is_empty()
    }
}

impl From<u32> for ChapterNumber {
    fn from(whole: u32) -> Self {
        Self {
            whole,
            frac: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid chapter number: {0:?}")]
pub struct ParseChapterNumberError(String);

impl FromStr for ChapterNumber {
    type Err = ParseChapterNumberError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let err = || ParseChapterNumberError(input.to_owned());
        let s = input.trim();
        // Some sites use a comma as the decimal separator.
        let (whole, frac) = s.split_once(['.', ',']).unwrap_or((s, ""));
        let all_digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
        if whole.is_empty() || !all_digits(whole) || !all_digits(frac) {
            return Err(err());
        }
        Ok(Self {
            whole: whole.parse().map_err(|_| err())?,
            frac: frac.trim_end_matches('0').to_owned(),
        })
    }
}

impl fmt::Display for ChapterNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.frac.is_empty() {
            write!(f, "{}", self.whole)
        } else {
            write!(f, "{}.{}", self.whole, self.frac)
        }
    }
}

impl Ord for ChapterNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        // Comparing the digit strings lexicographically is decimal ordering,
        // because trailing zeros were stripped: "15" < "5" ⇔ 0.15 < 0.5.
        self.whole
            .cmp(&other.whole)
            .then_with(|| self.frac.cmp(&other.frac))
    }
}

impl PartialOrd for ChapterNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Serialized as a string ("12.5") so JSON clients never see float rounding.
impl Serialize for ChapterNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ChapterNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = Cow::<'de, str>::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> ChapterNumber {
        s.parse().unwrap()
    }

    #[test]
    fn parses_and_normalizes() {
        assert_eq!(n("12").to_string(), "12");
        assert_eq!(n("012.50").to_string(), "12.5");
        assert_eq!(n(" 3,5 ").to_string(), "3.5");
        assert_eq!(n("7.").to_string(), "7");
        assert_eq!(n("12.50"), n("12.5"));
        assert!(n("4.0").is_whole());
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", ".5", "abc", "1.2.3", "-1", "1e3", "１２"] {
            assert!(
                bad.parse::<ChapterNumber>().is_err(),
                "{bad:?} should not parse"
            );
        }
    }

    #[test]
    fn orders_like_decimals() {
        let mut v = [n("10"), n("2"), n("2.5"), n("2.15"), n("0"), n("2.05")];
        v.sort();
        let sorted: Vec<String> = v.iter().map(ToString::to_string).collect();
        assert_eq!(sorted, ["0", "2", "2.05", "2.15", "2.5", "10"]);
    }

    #[test]
    fn serde_round_trip_as_string() {
        let json = serde_json::to_string(&n("1.5")).unwrap();
        assert_eq!(json, r#""1.5""#);
        let back: ChapterNumber = serde_json::from_str(&json).unwrap();
        assert_eq!(back, n("1.5"));
        assert!(serde_json::from_str::<ChapterNumber>(r#""x""#).is_err());
    }
}
