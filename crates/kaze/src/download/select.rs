//! Chapter selections like `1-10,12,15.5`, `latest` or `all`.

use std::str::FromStr;

use kaze_core::ChapterNumber;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    All,
    Latest,
    One(ChapterNumber),
    /// Inclusive on both ends.
    Range(ChapterNumber, ChapterNumber),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection(Vec<Part>);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid chapter selection {0:?}: use e.g. `5`, `1-10`, `1-3,7`, `latest` or `all`")]
pub struct SelectionError(String);

impl FromStr for Selection {
    type Err = SelectionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || SelectionError(s.to_owned());
        let parts = s
            .split(',')
            .map(str::trim)
            .map(|part| match part {
                "all" => Ok(Part::All),
                "latest" => Ok(Part::Latest),
                _ => match part.split_once('-') {
                    Some((lo, hi)) => Ok(Part::Range(
                        lo.trim().parse().map_err(|_| err())?,
                        hi.trim().parse().map_err(|_| err())?,
                    )),
                    None => part.parse().map(Part::One).map_err(|_| err()),
                },
            })
            .collect::<Result<Vec<_>, _>>()?;
        if parts.is_empty() {
            return Err(err());
        }
        Ok(Self(parts))
    }
}

impl Selection {
    /// Keep the items whose chapter number matches. `latest` means the
    /// highest number present in `items`.
    pub fn filter<'a, T>(
        &self,
        items: &'a [T],
        number: impl Fn(&T) -> &ChapterNumber,
    ) -> Vec<&'a T> {
        let latest = items.iter().map(&number).max();
        items
            .iter()
            .filter(|item| {
                let n = number(item);
                self.0.iter().any(|part| match part {
                    Part::All => true,
                    Part::Latest => Some(n) == latest,
                    Part::One(x) => n == x,
                    Part::Range(lo, hi) => lo <= n && n <= hi,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(spec: &str) -> Vec<String> {
        let items: Vec<ChapterNumber> = ["1", "2", "2.5", "3", "10"]
            .iter()
            .map(|n| n.parse().unwrap())
            .collect();
        let selection: Selection = spec.parse().unwrap();
        selection
            .filter(&items, |n| n)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn selects() {
        assert_eq!(numbers("2-3"), ["2", "2.5", "3"]);
        assert_eq!(numbers("1, 10"), ["1", "10"]);
        assert_eq!(numbers("latest"), ["10"]);
        assert_eq!(numbers("all").len(), 5);
        assert_eq!(numbers("4-9"), Vec::<String>::new());
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "x", "1-", "1,,2", "-3"] {
            assert!(bad.parse::<Selection>().is_err(), "{bad:?}");
        }
    }
}
