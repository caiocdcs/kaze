//! Turning a chapter on a source into a `.cbz` on disk.

mod cbz;
pub mod comicinfo;
mod image;
pub mod naming;
pub mod select;

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use futures_util::{StreamExt as _, TryStreamExt as _, stream};
use kaze_core::{ChapterInfo, ChapterKey, ChapterNumber, Series, Source, SourceError};
use url::Url;

pub use self::image::ImageFormat;
use self::{cbz::Page, naming::Template};

/// Everything known about a chapter that ends up in its file name and
/// `ComicInfo.xml`, independent of where it came from (source or database).
#[derive(Debug, Clone, Default)]
pub struct ChapterMeta {
    pub series_title: String,
    pub number: ChapterNumber,
    pub volume: Option<u32>,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
    pub year: Option<u16>,
    pub summary: Option<String>,
    pub language: Option<String>,
    pub url: Option<String>,
}

impl ChapterMeta {
    #[must_use]
    pub fn new(series: &Series, chapter: &ChapterInfo) -> Self {
        Self {
            series_title: series.title.clone(),
            number: chapter.number.clone(),
            volume: chapter.volume,
            title: chapter.title.clone(),
            authors: series.authors.clone(),
            tags: series.tags.clone(),
            year: series.year,
            summary: series.description.clone(),
            language: chapter.language.clone().or_else(|| series.language.clone()),
            url: Some(chapter.url.to_string()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error("page {page} is not an image ({url})")]
    NotAnImage { page: usize, url: Url },
    #[error("chapter has no pages")]
    NoPages,
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Downloaded {
        path: PathBuf,
        pages: usize,
    },
    /// A file with the target name already exists; nothing was fetched.
    AlreadyExists(PathBuf),
}

impl Outcome {
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Downloaded { path, .. } | Self::AlreadyExists(path) => path,
        }
    }
}

/// How a chapter download is carried out.
#[derive(Debug, Clone)]
pub struct Options {
    pub template: Template,
    /// Pages fetched at the same time (still bounded by the source's rate limit).
    pub concurrency: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            template: Template::default(),
            concurrency: 4,
        }
    }
}

/// Download one chapter into `dir` as a CBZ.
///
/// `progress(done, total)` is called after each page arrives.
pub async fn chapter(
    source: &dyn Source,
    key: &ChapterKey,
    meta: &ChapterMeta,
    dir: &Path,
    options: &Options,
    progress: &(dyn Fn(usize, usize) + Send + Sync),
) -> Result<Outcome, DownloadError> {
    let path = dir.join(format!("{}.cbz", options.template.render(meta)));
    if tokio::fs::try_exists(&path).await? {
        return Ok(Outcome::AlreadyExists(path));
    }

    let urls = source.pages(key).await?;
    if urls.is_empty() {
        return Err(DownloadError::NoPages);
    }

    // `buffered` runs up to N fetches at once but yields results in the
    // original order, so pages stay in reading order.
    let total = urls.len();
    let done = AtomicUsize::new(0);
    let pages: Vec<Page> = stream::iter(urls.iter().enumerate())
        .map(|(index, url)| async move {
            let image = source.fetch_image(url).await?;
            let format =
                ImageFormat::sniff(&image.bytes).ok_or_else(|| DownloadError::NotAnImage {
                    page: index + 1,
                    url: url.clone(),
                })?;
            Ok::<_, DownloadError>(Page {
                bytes: image.bytes,
                format,
            })
        })
        .buffered(options.concurrency.max(1))
        .inspect_ok(|_| progress(done.fetch_add(1, Ordering::Relaxed) + 1, total))
        .try_collect()
        .await?;

    tokio::fs::create_dir_all(dir).await?;
    let comic_info = comicinfo::render(meta, pages.len());
    // Write next to the target, then rename: the rename is atomic, so the
    // library never sees a half-written file.
    let partial = path.with_extension("cbz.part");
    let write_to = partial.clone();
    tokio::task::spawn_blocking(move || cbz::write(&write_to, &comic_info, &pages))
        .await
        .expect("cbz writer panicked")
        .map_err(|source| DownloadError::Write {
            path: partial.clone(),
            source,
        })?;
    tokio::fs::rename(&partial, &path).await?;

    Ok(Outcome::Downloaded { path, pages: total })
}

/// Save the series cover as `cover.<ext>` in `dir`, unless one exists.
pub async fn cover(
    source: &dyn Source,
    url: &Url,
    dir: &Path,
) -> Result<Option<PathBuf>, DownloadError> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir_all(dir).await?;
            tokio::fs::read_dir(dir).await?
        }
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_name().to_string_lossy().starts_with("cover.") {
            return Ok(None);
        }
    }

    let image = source.fetch_image(url).await?;
    let format = ImageFormat::sniff(&image.bytes).ok_or_else(|| DownloadError::NotAnImage {
        page: 0,
        url: url.clone(),
    })?;
    let path = dir.join(format!("cover.{}", format.extension()));
    tokio::fs::write(&path, &image.bytes).await?;
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use std::{io::Read as _, sync::Mutex};

    use async_trait::async_trait;
    use bytes::Bytes;
    use kaze_core::{Image, RateLimit, Result, SeriesHit, SeriesKey, SourceMeta, UrlTarget};

    use super::*;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

    /// A source that serves pages from memory.
    struct FakeSource {
        meta: SourceMeta,
        pages: Vec<(Url, &'static [u8])>,
        fetches: Mutex<usize>,
    }

    impl FakeSource {
        fn new(pages: &[&'static [u8]]) -> Self {
            Self {
                meta: SourceMeta {
                    id: "fake",
                    name: "Fake",
                    base_url: Url::parse("https://fake.test").unwrap(),
                    languages: &["en"],
                    rate_limit: RateLimit::per_second(100),
                },
                pages: pages
                    .iter()
                    .enumerate()
                    .map(|(i, b)| (Url::parse(&format!("https://fake.test/{i}")).unwrap(), *b))
                    .collect(),
                fetches: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl Source for FakeSource {
        fn meta(&self) -> &SourceMeta {
            &self.meta
        }
        fn parse_url(&self, _: &Url) -> Option<UrlTarget> {
            None
        }
        async fn search(&self, _: &str) -> Result<Vec<SeriesHit>> {
            Ok(vec![])
        }
        async fn series(&self, _: &SeriesKey) -> Result<Series> {
            unimplemented!()
        }
        async fn chapters(&self, _: &SeriesKey) -> Result<Vec<ChapterInfo>> {
            Ok(vec![])
        }
        async fn pages(&self, _: &ChapterKey) -> Result<Vec<Url>> {
            Ok(self.pages.iter().map(|(u, _)| u.clone()).collect())
        }
        async fn fetch_image(&self, url: &Url) -> Result<Image> {
            *self.fetches.lock().unwrap() += 1;
            let (_, bytes) = self.pages.iter().find(|(u, _)| u == url).unwrap();
            Ok(Image {
                bytes: Bytes::from_static(bytes),
                content_type: None,
            })
        }
    }

    fn meta() -> ChapterMeta {
        ChapterMeta {
            series_title: "Test".into(),
            number: "7".parse().unwrap(),
            ..ChapterMeta::default()
        }
    }

    #[tokio::test]
    async fn writes_a_cbz_with_sniffed_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let source = FakeSource::new(&[JPEG, PNG, JPEG]);
        let calls = Mutex::new(Vec::new());
        let outcome = chapter(
            &source,
            &"c".into(),
            &meta(),
            dir.path(),
            &Options::default(),
            &|done, total| calls.lock().unwrap().push((done, total)),
        )
        .await
        .unwrap();

        let path = dir.path().join("Test - Ch.7.cbz");
        assert_eq!(
            outcome,
            Outcome::Downloaded {
                path: path.clone(),
                pages: 3
            }
        );
        assert_eq!(*calls.lock().unwrap(), [(1, 3), (2, 3), (3, 3)]);
        assert!(!dir.path().join("Test - Ch.7.cbz.part").exists());

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let names: Vec<_> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
        assert_eq!(names, ["ComicInfo.xml", "001.jpg", "002.png", "003.jpg"]);
        let mut xml = String::new();
        zip.by_name("ComicInfo.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("<PageCount>3</PageCount>"));
    }

    #[tokio::test]
    async fn existing_file_is_not_downloaded_again() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Test - Ch.7.cbz"), b"x").unwrap();
        let source = FakeSource::new(&[JPEG]);
        let outcome = chapter(
            &source,
            &"c".into(),
            &meta(),
            dir.path(),
            &Options::default(),
            &|_, _| {},
        )
        .await
        .unwrap();
        assert!(matches!(outcome, Outcome::AlreadyExists(_)));
        assert_eq!(*source.fetches.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn html_instead_of_image_fails_without_leaving_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = FakeSource::new(&[JPEG, b"<html>rate limited</html>"]);
        let err = chapter(
            &source,
            &"c".into(),
            &meta(),
            dir.path(),
            &Options::default(),
            &|_, _| {},
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, DownloadError::NotAnImage { page: 2, .. }),
            "{err}"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn saves_cover_once() {
        let dir = tempfile::tempdir().unwrap();
        let source = FakeSource::new(&[PNG]);
        let url = source.pages[0].0.clone();
        let saved = cover(&source, &url, &dir.path().join("Series"))
            .await
            .unwrap();
        assert_eq!(saved.unwrap().file_name().unwrap(), "cover.png");
        assert_eq!(
            cover(&source, &url, &dir.path().join("Series"))
                .await
                .unwrap(),
            None
        );
    }
}
