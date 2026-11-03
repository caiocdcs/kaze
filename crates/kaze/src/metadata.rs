//! Finding a series' AniList entry by title. The rules come from a trial
//! on live titles from every source: no wrong links, so a series stays
//! unlinked rather than get a doubtful one.

use kaze_core::{AniListId, Result};
use kaze_sources::anilist::{AniList, Format, Media};

/// A close match must score at least this (Jaro-Winkler)…
const THRESHOLD: f64 = 0.92;
/// …and lead the next candidate by this much.
const MARGIN: f64 = 0.03;
/// Among exact matches, the most popular wins only by this factor.
const POPULAR_BY: u32 = 5;
/// AniList's search finds nothing for some long titles; a short head does.
const SHORT_QUERY_WORDS: usize = 6;

/// How a link was found, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// A title or synonym equals the source's title.
    Exact,
    /// Several do; this one is far more popular.
    ExactMostPopular,
    /// Similar enough and clearly ahead of the rest.
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Found {
        id: AniListId,
        how: How,
    },
    /// Candidates too alike to choose: left for linking by hand.
    Ambiguous,
    NotFound,
}

/// Search AniList for a source title, retrying with shorter forms of it,
/// and pick a match by the rules above.
pub async fn find(anilist: &AniList, title: &str) -> Result<Outcome> {
    let mut hits: Vec<Media> = Vec::new();
    let mut outcome = Outcome::NotFound;
    for query in queries(title) {
        for media in anilist.search(&query).await? {
            if hits.iter().all(|h| h.id != media.id) {
                hits.push(media);
            }
        }
        outcome = pick(title, &hits);
        if matches!(outcome, Outcome::Found { .. }) {
            break;
        }
    }
    Ok(outcome)
}

/// The title, then without its subtitle, then its first words.
fn queries(title: &str) -> Vec<String> {
    let words: Vec<&str> = title.split_whitespace().collect();
    let short = (words.len() > SHORT_QUERY_WORDS).then(|| {
        words[..SHORT_QUERY_WORDS]
            .join(" ")
            .trim_end_matches(|c: char| !c.is_alphanumeric())
            .to_owned()
    });
    let mut out: Vec<String> = Vec::new();
    for query in [
        Some(title.to_owned()),
        head(title).map(ToOwned::to_owned),
        short,
    ]
    .into_iter()
    .flatten()
    {
        if !out.contains(&query) {
            out.push(query);
        }
    }
    out
}

fn pick(title: &str, hits: &[Media]) -> Outcome {
    let wanted = normalize(title);
    let exact: Vec<&Media> = hits
        .iter()
        .filter(|m| names(m).any(|n| normalize(n) == wanted))
        .collect();
    if !exact.is_empty() {
        // A one-shot or pilot often shares the series' title.
        let series: Vec<&Media> = exact
            .iter()
            .copied()
            .filter(|m| m.format != Some(Format::OneShot))
            .collect();
        let mut candidates = if series.is_empty() { exact } else { series };
        if let [only] = candidates.as_slice() {
            return Outcome::Found {
                id: only.id,
                how: How::Exact,
            };
        }
        candidates.sort_by_key(|m| std::cmp::Reverse(m.popularity));
        let (first, second) = (candidates[0], candidates[1]);
        if first.popularity >= second.popularity.max(1).saturating_mul(POPULAR_BY) {
            return Outcome::Found {
                id: first.id,
                how: How::ExactMostPopular,
            };
        }
        return Outcome::Ambiguous;
    }

    let mut scored: Vec<(f64, AniListId)> = hits
        .iter()
        .map(|m| (similarity(&wanted, m), m.id))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    match scored.as_slice() {
        [] => Outcome::NotFound,
        [(best, _), ..] if *best < THRESHOLD => Outcome::NotFound,
        [(best, id), rest @ ..] => {
            let second = rest.first().map_or(0.0, |r| r.0);
            if best - second >= MARGIN {
                Outcome::Found {
                    id: *id,
                    how: How::Close,
                }
            } else {
                Outcome::Ambiguous
            }
        }
    }
}

/// Best score against each name, and each name without its subtitle
/// ("Übel Blatt II" against "Übel Blatt II: Knights of the Fallen King").
fn similarity(wanted: &str, media: &Media) -> f64 {
    names(media)
        .flat_map(|n| std::iter::once(n).chain(head(n)))
        .map(|n| strsim::jaro_winkler(wanted, &normalize(n)))
        .fold(0.0, f64::max)
}

fn names(media: &Media) -> impl Iterator<Item = &str> {
    let t = &media.titles;
    [&t.romaji, &t.english]
        .into_iter()
        .flatten()
        .chain(&media.synonyms)
        .map(String::as_str)
}

/// The part before a subtitle separator, if there is one.
fn head(title: &str) -> Option<&str> {
    [":", " ~", " - ", " – "]
        .iter()
        .find_map(|sep| title.split_once(sep))
        .map(|(head, _)| head.trim())
        .filter(|head| !head.is_empty())
}

/// Lower case, accents folded (Übel → ubel), punctuation as spaces, and
/// no leading article: "The Player Who Can't Level Up" and "Player Who
/// Can’t Level-Up" compare equal.
fn normalize(title: &str) -> String {
    let folded: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() { fold(c) } else { ' ' })
        .flat_map(char::to_lowercase)
        .collect();
    let words: Vec<&str> = folded.split_whitespace().collect();
    let words = match words.as_slice() {
        ["the" | "a" | "an", rest @ ..] if !rest.is_empty() => rest,
        all => all,
    };
    words.join(" ")
}

/// Latin letters with diacritics to their base letter. Titles are mostly
/// English or romanized, so a table beats pulling in Unicode normalization.
fn fold(c: char) -> char {
    match c {
        'À'..='Å' | 'à'..='å' | 'Ā' | 'ā' => 'a',
        'Ç' | 'ç' => 'c',
        'È'..='Ë' | 'è'..='ë' | 'Ē' | 'ē' => 'e',
        'Ì'..='Ï' | 'ì'..='ï' | 'Ī' | 'ī' => 'i',
        'Ñ' | 'ñ' => 'n',
        'Ò'..='Ö' | 'ò'..='ö' | 'Ø' | 'ø' | 'Ō' | 'ō' => 'o',
        'Ù'..='Ü' | 'ù'..='ü' | 'Ū' | 'ū' => 'u',
        'Ý' | 'ý' | 'ÿ' => 'y',
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use kaze_core::SeriesStatus;
    use kaze_sources::anilist::Titles;

    use super::*;

    fn media(id: u32, romaji: &str, english: Option<&str>, synonyms: &[&str]) -> Media {
        Media {
            id: AniListId(id),
            format: Some(Format::Manga),
            popularity: 100,
            titles: Titles {
                romaji: Some(romaji.into()),
                english: english.map(Into::into),
                native: None,
            },
            synonyms: synonyms.iter().map(|&s| s.into()).collect(),
            description: None,
            genres: vec![],
            tags: vec![],
            status: SeriesStatus::Unknown,
            year: None,
            cover_url: None,
            staff: vec![],
        }
    }

    fn found(id: u32, how: How) -> Outcome {
        Outcome::Found {
            id: AniListId(id),
            how,
        }
    }

    #[test]
    fn normalizes_titles() {
        assert_eq!(normalize("Übel Blatt II"), "ubel blatt ii");
        assert_eq!(
            normalize("The Player Who Can't Level-Up"),
            "player who can t level up"
        );
        assert_eq!(
            normalize("Player Who Can’t Level Up"),
            "player who can t level up"
        );
        assert_eq!(normalize("The"), "the", "a lone article stays");
        assert_eq!(head("Frieren: Beyond Journey's End"), Some("Frieren"));
        assert_eq!(head("Dandadan"), None);
    }

    #[test]
    fn exact_beats_close() {
        // "Jujutsu Kaisen 0" scores 0.975 against "Jujutsu Kaisen": too close
        // for the margin, but only one is exact.
        let hits = [
            media(101_517, "Jujutsu Kaisen", Some("Jujutsu Kaisen"), &[]),
            media(105_469, "Jujutsu Kaisen 0", Some("Jujutsu Kaisen 0"), &[]),
        ];
        assert_eq!(pick("Jujutsu Kaisen", &hits), found(101_517, How::Exact));
    }

    #[test]
    fn scanlation_names_match_synonyms() {
        let hits = [media(
            1,
            "Gwonwangjeonsaeng",
            Some("The Reincarnated King of Fists"),
            &["The Indomitable Martial King"],
        )];
        assert_eq!(
            pick("The Indomitable Martial King", &hits),
            found(1, How::Exact)
        );
    }

    #[test]
    fn one_shot_and_popularity_break_exact_ties() {
        let mut oneshot = media(86_813, "Komi-san wa Komyushou desu.", None, &[]);
        oneshot.format = Some(Format::OneShot);
        let series = media(97_852, "Komi-san wa, Komyushou desu.", None, &[]);
        assert_eq!(
            pick("Komi-san wa Komyushou Desu.", &[oneshot, series]),
            found(97_852, How::Exact)
        );

        let mut popular = media(30_013, "ONE PIECE", Some("One Piece"), &[]);
        popular.popularity = 100_000;
        let other = media(95_552, "Wan Piece", None, &["One Piece"]);
        assert_eq!(
            pick("One Piece", &[other.clone(), popular]),
            found(30_013, How::ExactMostPopular)
        );

        let twin = media(165_265, "One Piece", None, &[]);
        assert_eq!(pick("One Piece", &[other, twin]), Outcome::Ambiguous);
    }

    #[test]
    fn close_needs_a_lead() {
        let hits = [media(
            177_816,
            "Übel Blatt II: Shiseru Ou no Kishidan",
            Some("Übel Blatt II: Knights of the Fallen King"),
            &[],
        )];
        assert_eq!(pick("Ubel Blatt II", &hits), found(177_816, How::Close));

        let tie = [
            media(1, "Kimi no Kami ga Ki ni Naru", None, &[]),
            media(2, "Kimi no Kami ga Ki ni Narunde", None, &[]),
        ];
        assert_eq!(
            pick("Kimi no Kami ga Ki ni Narunda", &tie),
            Outcome::Ambiguous
        );
        assert_eq!(pick("Something Else", &tie), Outcome::NotFound);
        assert_eq!(pick("Anything", &[]), Outcome::NotFound);
    }

    #[test]
    fn retries_shorter_queries() {
        assert_eq!(
            queries("Isekai Kenja no Tensei Musou: Game no Chishiki de"),
            [
                "Isekai Kenja no Tensei Musou: Game no Chishiki de",
                "Isekai Kenja no Tensei Musou",
                "Isekai Kenja no Tensei Musou: Game",
            ]
        );
        assert_eq!(queries("Dandadan"), ["Dandadan"]);
        assert_eq!(
            queries("I went from the strongest job, Dragon Knight, to a beginner"),
            [
                "I went from the strongest job, Dragon Knight, to a beginner",
                "I went from the strongest job",
            ]
        );
    }
}
