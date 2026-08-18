pub mod chapter;
pub mod library;
pub mod manga;

pub use chapter::ChapterService;
pub use library::LibraryService;
pub use manga::MangaService;

use std::sync::Arc;

use crate::domain::chapter::ChapterRepository;
use crate::domain::library::LibraryRepository;
use crate::domain::manga::MangaRepository;
use crate::infra::connectors::SourceRegistry;

/// Container of all application services for easy wiring.
pub struct Services {
    pub manga: MangaService,
    pub chapter: ChapterService,
    pub library: LibraryService,
    pub sources: Arc<SourceRegistry>,
}

impl Services {
    pub fn new(
        manga_repo: Arc<dyn MangaRepository>,
        chapter_repo: Arc<dyn ChapterRepository>,
        library_repo: Arc<dyn LibraryRepository>,
        sources: Arc<SourceRegistry>,
    ) -> Self {
        Self {
            manga: MangaService::new(
                manga_repo,
                chapter_repo.clone(),
                library_repo.clone(),
                sources.clone(),
            ),
            chapter: ChapterService::new(chapter_repo),
            library: LibraryService::new(library_repo),
            sources,
        }
    }
}
