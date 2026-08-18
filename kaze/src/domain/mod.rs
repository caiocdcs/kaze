pub mod chapter;
pub mod error;
pub mod library;
pub mod library_provider;
pub mod manga;
pub mod manga_source;

pub use chapter::{Chapter, ChapterRepository, ChapterStatus};
pub use error::DomainError;
pub use library::{FileLibrary, LibraryRepository};
pub use library_provider::LibraryProvider;
pub use manga::{ConnectorMapping, Manga, MangaRepository, ReleaseStatus};
pub use manga_source::{ChapterCandidate, MangaCandidate, MangaSource};

/// Re-exported for convenience -- every domain module defines its own DomainResult.
pub type DomainResult<T> = Result<T, error::DomainError>;
