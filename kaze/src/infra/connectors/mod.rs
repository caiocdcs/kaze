pub mod chromium;
pub mod mangadex;
pub mod weeb_central;

pub use mangadex::MangaDexSource;
pub use weeb_central::WeebCentralSource;

use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::manga_source::MangaSource;

/// Registry of all available manga sources.
pub struct SourceRegistry {
    sources: HashMap<String, Arc<dyn MangaSource>>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
        }
    }

    pub fn register(&mut self, source: Arc<dyn MangaSource>) {
        let name = source.name().to_lowercase();
        self.sources.insert(name, source);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn MangaSource>> {
        self.sources.get(&name.to_lowercase()).cloned()
    }

    pub fn all(&self) -> Vec<Arc<dyn MangaSource>> {
        self.sources.values().cloned().collect()
    }

    pub fn find_by_url(&self, url: &str) -> Option<Arc<dyn MangaSource>> {
        self.sources.values().find(|s| s.matches_url(url)).cloned()
    }
}
