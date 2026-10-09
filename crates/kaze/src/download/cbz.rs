//! Writing `.cbz` files (a zip of images plus `ComicInfo.xml`).

use std::{
    fs::File,
    io::{BufWriter, Write as _},
    path::Path,
};

use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use super::image::ImageFormat;

/// One page ready to be archived.
#[derive(Debug)]
pub struct Page {
    pub bytes: bytes::Bytes,
    pub format: ImageFormat,
}

/// Write a CBZ to `path`. Blocking: call it from `spawn_blocking`.
///
/// Pages are stored uncompressed (images are already compressed) and
/// named `001.jpg`, `002.png`, ... so every reader sorts them correctly.
pub fn write(path: &Path, comic_info: &str, pages: &[Page]) -> zip::result::ZipResult<()> {
    let mut zip = ZipWriter::new(BufWriter::new(File::create(path)?));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    zip.start_file("ComicInfo.xml", options)?;
    zip.write_all(comic_info.as_bytes())?;

    let width = pages.len().to_string().len().max(3);
    for (index, page) in pages.iter().enumerate() {
        let name = format!("{:0width$}.{}", index + 1, page.format.extension());
        zip.start_file(name, options)?;
        zip.write_all(&page.bytes)?;
    }

    let mut writer = zip.finish()?;
    writer.flush()?;
    // Make sure the data is on disk before the caller renames the file
    // into place, so a crash never leaves a truncated .cbz behind.
    writer.get_ref().sync_all()?;
    Ok(())
}
