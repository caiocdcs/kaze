use std::path::PathBuf;
use tokio::fs;

/// Cache sizes: (name, max_width, max_height, jpeg_quality)
const CACHE_SIZES: &[(&str, u32, u32, u8)] = &[
    ("large", 900, 1280, 40),
    ("medium", 450, 640, 40),
    ("small", 225, 320, 40),
];

pub struct CoverCache {
    pub original_dir: PathBuf,
    pub base_dir: PathBuf,
}

impl CoverCache {
    pub fn new(base_dir: PathBuf) -> Self {
        let original_dir = base_dir.join("original");
        Self { original_dir, base_dir }
    }

    pub async fn ensure_dirs(&self) -> Result<(), std::io::Error> {
        fs::create_dir_all(&self.original_dir).await?;
        for (name, _, _, _) in CACHE_SIZES {
            fs::create_dir_all(self.base_dir.join(name)).await?;
        }
        Ok(())
    }

    pub fn cache_filename(&self, domain: &str, id: &str, connector: &str, ext: &str) -> String {
        format!("{domain}-{id}.{connector}.{ext}")
    }

    pub async fn save_original(&self, filename: &str, data: &[u8]) -> Result<PathBuf, std::io::Error> {
        let path = self.original_dir.join(filename);
        fs::write(&path, data).await?;
        Ok(path)
    }

    pub async fn generate_cached_sizes(&self, filename: &str, data: &[u8]) -> Result<(), std::io::Error> {
        for (size_name, max_w, max_h, quality) in CACHE_SIZES {
            if let Some(resized) = crate::image::resize_cover(data, *max_w, *max_h, *quality) {
                let path = self.base_dir.join(size_name).join(filename);
                fs::write(&path, resized).await?;
            }
        }
        Ok(())
    }

    pub async fn get_cover(
        &self,
        size: &str,
        filename: &str,
    ) -> Option<Vec<u8>> {
        let dir = if size == "original" {
            self.original_dir.clone()
        } else {
            self.base_dir.join(size)
        };
        let path = dir.join(filename);
        (fs::read(&path).await).ok()
    }

    pub async fn cleanup_unused(&self, used_files: &[String]) -> Result<u64, std::io::Error> {
        let mut removed = 0u64;
        for (size_name, _, _, _) in CACHE_SIZES {
            let dir = self.base_dir.join(size_name);
            if !dir.exists() {
                continue;
            }
            let mut entries = fs::read_dir(&dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let name = entry.file_name().to_string_lossy().to_string();
                if !used_files.iter().any(|u| u == &name) {
                    fs::remove_file(entry.path()).await?;
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
}
