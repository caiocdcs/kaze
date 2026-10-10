//! Importing an existing library (from Tranga, or by hand): find series
//! folders kaze doesn't track, work out which chapters they hold, and
//! suggest which source series each one is.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Read as _,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use kaze_core::{ChapterNumber, SeriesHit};
use regex::Regex;
use serde::Serialize;
use ts_rs::TS;

/// A folder in the library that no followed series uses.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct Candidate {
    pub folder: String,
    /// `.cbz` files in the folder.
    pub files: usize,
    /// Distinct chapter numbers found, ascending.
    pub chapters: Vec<ChapterNumber>,
    /// Files whose chapter number couldn't be worked out.
    pub unparsed: usize,
}

/// A source series that may be what a folder holds.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Match {
    pub source: String,
    pub hit: SeriesHit,
    /// 0 to 1; how close the titles are.
    pub score: f64,
}

/// Folders under `library` not in `followed`, with their chapters. Blocking IO.
pub fn scan<S: std::hash::BuildHasher>(
    library: &Path,
    followed: &HashSet<String, S>,
) -> std::io::Result<Vec<Candidate>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(library)? {
        let entry = entry?;
        let Ok(folder) = entry.file_name().into_string() else {
            continue; // not UTF-8; can't be stored as a folder name
        };
        if !entry.file_type()?.is_dir() || folder.starts_with('.') || followed.contains(&folder) {
            continue;
        }
        let files = cbz_files(&entry.path())?;
        if files.is_empty() {
            continue;
        }
        let found = chapter_files(&files);
        let unparsed = files.len() - found.values().map(Vec::len).sum::<usize>();
        out.push(Candidate {
            folder,
            files: files.len(),
            chapters: found.into_keys().collect(),
            unparsed,
        });
    }
    out.sort_by_key(|c| c.folder.to_lowercase());
    Ok(out)
}

pub fn cbz_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let is_cbz = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("cbz"));
        if is_cbz && path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Chapter number → the files holding it. ComicInfo wins over the name.
#[must_use]
pub fn chapter_files(files: &[PathBuf]) -> BTreeMap<ChapterNumber, Vec<PathBuf>> {
    let mut found: BTreeMap<ChapterNumber, Vec<PathBuf>> = BTreeMap::new();
    for path in files {
        let number = comicinfo_number(path).or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .and_then(number_from_name)
        });
        if let Some(number) = number {
            found.entry(number).or_default().push(path.clone());
        }
    }
    found
}

/// `<Number>` from the archive's ComicInfo.xml, if it has one.
fn comicinfo_number(path: &Path) -> Option<ChapterNumber> {
    let file = fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("ComicInfo.xml").ok()?;
    let mut xml = String::new();
    // ComicInfo is small; refuse anything absurd rather than read it all.
    entry.by_ref().take(1 << 20).read_to_string(&mut xml).ok()?;
    let start = xml.find("<Number>")? + "<Number>".len();
    let end = start + xml[start..].find("</Number>")?;
    xml[start..end].trim().parse().ok()
}

#[allow(clippy::unwrap_used, reason = "literal patterns, covered by tests")]
static CHAPTER_PATTERNS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [
        // "Ch.12", "Ch 12.5", "Chapter 12", "chap12"
        Regex::new(r"(?i)\bch(?:apter|ap)?\.?\s*(\d+(?:[.,]\d+)?)").unwrap(),
        // "c012", "#12"
        Regex::new(r"(?i)(?:\bc|#)(\d+(?:\.\d+)?)\b").unwrap(),
        // otherwise the last number in the name: "One Piece 1096"
        Regex::new(r"(\d+(?:\.\d+)?)\D*$").unwrap(),
    ]
});

fn number_from_name(name: &str) -> Option<ChapterNumber> {
    CHAPTER_PATTERNS
        .iter()
        .find_map(|re| re.captures(name))
        .and_then(|c| c[1].parse().ok())
}

/// Lower case, letters and digits only: "Frieren: Beyond Journey's End"
/// and "Frieren - Beyond Journeys End" compare equal.
fn normalize(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Score search hits against a folder name, best first.
#[must_use]
pub fn rank(folder: &str, hits: Vec<(String, SeriesHit)>) -> Vec<Match> {
    let wanted = normalize(folder);
    let mut matches: Vec<Match> = hits
        .into_iter()
        .map(|(source, hit)| {
            let score = strsim::jaro_winkler(&wanted, &normalize(&hit.title));
            Match { source, hit, score }
        })
        .collect();
    matches.sort_by(|a, b| b.score.total_cmp(&a.score));
    matches
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn n(s: &str) -> ChapterNumber {
        s.parse().unwrap()
    }

    #[test]
    fn chapter_numbers_from_file_names() {
        let cases = [
            ("One Piece - Vol.3 Ch.25 - Title", "25"),
            ("Berserk Ch 12.5", "12.5"),
            ("Frieren Chapter 140", "140"),
            ("Dandadan c012", "12"),
            ("Vagabond 327", "327"),
            ("Series - Ch.1,5", "1.5"),
        ];
        for (name, want) in cases {
            assert_eq!(number_from_name(name), Some(n(want)), "{name}");
        }
        assert_eq!(number_from_name("cover"), None);
    }

    fn cbz(path: &Path, comicinfo: Option<&str>) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        if let Some(number) = comicinfo {
            zip.start_file("ComicInfo.xml", options).unwrap();
            write!(zip, "<ComicInfo><Number>{number}</Number></ComicInfo>").unwrap();
        }
        zip.start_file("001.jpg", options).unwrap();
        zip.write_all(b"jpg").unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn scans_unfollowed_folders() {
        let lib = tempfile::tempdir().unwrap();
        let berserk = lib.path().join("Berserk");
        fs::create_dir(&berserk).unwrap();
        // ComicInfo says 10.5 even though the name says 99.
        cbz(&berserk.join("Berserk Ch.99.cbz"), Some("10.5"));
        cbz(&berserk.join("Berserk Ch.11.cbz"), None);
        cbz(&berserk.join("extras.cbz"), None);
        fs::write(berserk.join("cover.jpg"), b"x").unwrap();

        fs::create_dir(lib.path().join("Followed")).unwrap();
        cbz(&lib.path().join("Followed/Ch.1.cbz"), None);
        fs::create_dir(lib.path().join("Empty")).unwrap();

        let followed = HashSet::from(["Followed".to_owned()]);
        let found = scan(lib.path(), &followed).unwrap();
        assert_eq!(
            found,
            [Candidate {
                folder: "Berserk".into(),
                files: 3,
                chapters: vec![n("10.5"), n("11")],
                unparsed: 1,
            }]
        );
    }

    #[test]
    fn ranks_close_titles_first() {
        let hit = |title: &str| SeriesHit {
            key: title.into(),
            title: title.into(),
            url: "https://x.test/s".parse().unwrap(),
            cover_url: None,
        };
        let ranked = rank(
            "Frieren - Beyond Journeys End",
            vec![
                ("a".into(), hit("Frieren Specials")),
                ("a".into(), hit("Frieren: Beyond Journey's End")),
            ],
        );
        assert_eq!(ranked[0].hit.title, "Frieren: Beyond Journey's End");
        assert!(ranked[0].score > 0.99, "{}", ranked[0].score);
    }
}
