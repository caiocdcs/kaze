use async_trait::async_trait;
use sqlx::sqlite::SqlitePool;

use crate::domain::DomainResult;
use crate::domain::error::{DomainError, StorageError, StorageKind};
use crate::domain::manga::*;
use crate::id::EntityId;

pub struct SqliteMangaRepository {
    pool: SqlitePool,
}

impl SqliteMangaRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn map_sqlx_error(e: sqlx::Error) -> DomainError {
        DomainError::Storage(StorageError {
            kind: StorageKind::Query,
            detail: e.to_string(),
        })
    }

    async fn get_connectors(&self, manga_id: &str) -> DomainResult<Vec<ConnectorMapping>> {
        let rows = sqlx::query_as::<_, ConnectorMappingRow>(
            "SELECT key, manga_connector_name, id_on_connector_site, website_url, use_for_download FROM manga_connector_to_manga WHERE obj_id = $1"
        )
        .bind(manga_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|r| ConnectorMapping {
                id: r.key,
                connector_name: r.manga_connector_name,
                site_id: r.id_on_connector_site,
                website_url: r.website_url,
                use_for_download: r.use_for_download == 1,
            })
            .collect())
    }

    async fn get_authors(&self, manga_id: &str) -> DomainResult<Vec<Author>> {
        let rows = sqlx::query_as::<_, AuthorRow>(
            r#"SELECT a.key, a.author_name
               FROM authors a
               INNER JOIN manga_authors ma ON ma.author_key = a.key
               WHERE ma.manga_key = $1"#,
        )
        .bind(manga_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|r| Author {
                id: r.key,
                name: r.author_name,
            })
            .collect())
    }

    async fn get_tags(&self, manga_id: &str) -> DomainResult<Vec<String>> {
        let rows = sqlx::query_as::<_, TagRow>(
            r#"SELECT t.tag FROM manga_tags t
               INNER JOIN manga_tag_associations mt ON mt.tag = t.tag
               WHERE mt.manga_key = $1"#,
        )
        .bind(manga_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(rows.into_iter().map(|r| r.tag).collect())
    }

    async fn get_links(&self, manga_id: &str) -> DomainResult<Vec<Link>> {
        let rows = sqlx::query_as::<_, LinkRow>(
            "SELECT link_provider, link_url FROM links WHERE manga_key = $1",
        )
        .bind(manga_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|r| Link {
                provider: r.link_provider,
                url: r.link_url,
            })
            .collect())
    }

    async fn get_alt_titles(&self, manga_id: &str) -> DomainResult<Vec<AltTitle>> {
        let rows = sqlx::query_as::<_, AltTitleRow>(
            "SELECT language, title FROM alt_titles WHERE manga_key = $1",
        )
        .bind(manga_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|r| AltTitle {
                language: r.language,
                title: r.title,
            })
            .collect())
    }

    /// Fetch all related data and build a domain Manga from the root row.
    async fn build_manga(&self, row: MangaRow) -> DomainResult<Manga> {
        let connectors = self.get_connectors(&row.key).await?;
        let authors = self.get_authors(&row.key).await?;
        let tags = self.get_tags(&row.key).await?;
        let links = self.get_links(&row.key).await?;
        let alt_titles = self.get_alt_titles(&row.key).await?;

        Ok(Manga {
            id: row.key,
            name: row.name,
            description: row.description,
            cover_url: row.cover_url,
            release_status: ReleaseStatus::from(row.release_status.as_str()),
            year: row.year.map(|y| y as u32),
            original_language: row.original_language,
            connectors,
            authors,
            tags,
            links,
            alt_titles,
            library_id: row.library_id,
            ignore_chapters_before: row.ignore_chapters_before,
        })
    }
}

#[async_trait]
impl MangaRepository for SqliteMangaRepository {
    async fn list(&self) -> DomainResult<Vec<Manga>> {
        let rows = sqlx::query_as::<_, MangaRow>(
            "SELECT key, name, description, cover_url, release_status, library_id, ignore_chapters_before, year, original_language FROM mangas ORDER BY name"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            result.push(self.build_manga(row).await?);
        }
        Ok(result)
    }

    async fn list_downloading(&self) -> DomainResult<Vec<Manga>> {
        let rows = sqlx::query_as::<_, MangaRow>(
            r#"SELECT m.key, m.name, m.description, m.cover_url, m.release_status, m.library_id, m.ignore_chapters_before, m.year, m.original_language
               FROM mangas m
               WHERE EXISTS (SELECT 1 FROM manga_connector_to_manga mc WHERE mc.obj_id = m.key AND mc.use_for_download = 1)
               ORDER BY m.name"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            result.push(self.build_manga(row).await?);
        }
        Ok(result)
    }

    async fn find_by_id(&self, id: &str) -> DomainResult<Option<Manga>> {
        let row = sqlx::query_as::<_, MangaRow>(
            "SELECT key, name, description, cover_url, release_status, library_id, ignore_chapters_before, year, original_language FROM mangas WHERE key = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        match row {
            Some(r) => Ok(Some(self.build_manga(r).await?)),
            None => Ok(None),
        }
    }

    async fn find_by_name(&self, name: &str) -> DomainResult<Option<String>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT key FROM mangas WHERE name = $1 LIMIT 1")
                .bind(name)
                .fetch_optional(&self.pool)
                .await
                .map_err(Self::map_sqlx_error)?;

        Ok(row.map(|r| r.0))
    }

    async fn list_names(&self) -> DomainResult<Vec<(String, String)>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT key, name FROM mangas ORDER BY name")
                .fetch_all(&self.pool)
                .await
                .map_err(Self::map_sqlx_error)?;
        Ok(rows)
    }

    async fn save(&self, manga: &Manga) -> DomainResult<()> {
        sqlx::query(
            r#"INSERT INTO mangas (key, name, description, cover_url, release_status, library_id, ignore_chapters_before, year, original_language)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
               ON CONFLICT (key) DO UPDATE SET
               name = $2, description = $3, cover_url = $4, release_status = $5,
               library_id = $6, ignore_chapters_before = $7, year = $8, original_language = $9"#
        )
        .bind(&manga.id)
        .bind(&manga.name)
        .bind(&manga.description)
        .bind(&manga.cover_url)
        .bind(manga.release_status.as_str())
        .bind(&manga.library_id)
        .bind(manga.ignore_chapters_before)
        .bind(manga.year.map(|y| y as i32))
        .bind(&manga.original_language)
        .execute(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> DomainResult<bool> {
        let result = sqlx::query("DELETE FROM mangas WHERE key = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn set_library(&self, manga_id: &str, library_id: Option<&str>) -> DomainResult<()> {
        sqlx::query("UPDATE mangas SET library_id = $1 WHERE key = $2")
            .bind(library_id)
            .bind(manga_id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn set_ignore_before(&self, manga_id: &str, threshold: f64) -> DomainResult<()> {
        sqlx::query("UPDATE mangas SET ignore_chapters_before = $1 WHERE key = $2")
            .bind(threshold)
            .bind(manga_id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn set_connector_download(
        &self,
        manga_id: &str,
        connector: &str,
        enabled: bool,
    ) -> DomainResult<bool> {
        let result = sqlx::query(
            "UPDATE manga_connector_to_manga SET use_for_download = $3 WHERE obj_id = $1 AND manga_connector_name = $2"
        )
        .bind(manga_id)
        .bind(connector)
        .bind(if enabled { 1 } else { 0 })
        .execute(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn add_connector(
        &self,
        manga_id: &str,
        connector: &str,
        site_id: &str,
        website_url: Option<&str>,
    ) -> DomainResult<ConnectorMapping> {
        let key = crate::id::ConnectorMappingId::new().to_string();
        sqlx::query(
            r#"INSERT INTO manga_connector_to_manga (key, obj_id, manga_connector_name, id_on_connector_site, website_url, use_for_download)
               VALUES ($1, $2, $3, $4, $5, 0)"#
        )
        .bind(&key)
        .bind(manga_id)
        .bind(connector)
        .bind(site_id)
        .bind(website_url)
        .execute(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(ConnectorMapping {
            id: key,
            connector_name: connector.to_string(),
            site_id: site_id.to_string(),
            website_url: website_url.map(String::from),
            use_for_download: false,
        })
    }

    async fn get_connector(
        &self,
        manga_id: &str,
        connector: &str,
    ) -> DomainResult<Option<ConnectorMapping>> {
        let row = sqlx::query_as::<_, ConnectorMappingRow>(
            "SELECT key, manga_connector_name, id_on_connector_site, website_url, use_for_download FROM manga_connector_to_manga WHERE obj_id = $1 AND manga_connector_name = $2"
        )
        .bind(manga_id)
        .bind(connector)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(row.map(|r| ConnectorMapping {
            id: r.key,
            connector_name: r.manga_connector_name,
            site_id: r.id_on_connector_site,
            website_url: r.website_url,
            use_for_download: r.use_for_download == 1,
        }))
    }

    async fn upsert_author(&self, name: &str) -> DomainResult<String> {
        // Check if author exists
        if let Some(row) =
            sqlx::query_as::<_, (String,)>("SELECT key FROM authors WHERE author_name = $1 LIMIT 1")
                .bind(name)
                .fetch_optional(&self.pool)
                .await
                .map_err(Self::map_sqlx_error)?
        {
            return Ok(row.0);
        }
        let key = crate::id::AuthorId::new().to_string();
        sqlx::query("INSERT INTO authors (key, author_name) VALUES ($1, $2)")
            .bind(&key)
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(key)
    }

    async fn link_author(&self, manga_id: &str, author_id: &str) -> DomainResult<()> {
        sqlx::query("INSERT OR IGNORE INTO manga_authors (manga_key, author_key) VALUES ($1, $2)")
            .bind(manga_id)
            .bind(author_id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn upsert_tag(&self, name: &str) -> DomainResult<()> {
        sqlx::query("INSERT OR IGNORE INTO manga_tags (tag) VALUES ($1)")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn link_tag(&self, manga_id: &str, tag: &str) -> DomainResult<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO manga_tag_associations (manga_key, tag) VALUES ($1, $2)",
        )
        .bind(manga_id)
        .bind(tag)
        .execute(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn upsert_alt_title(&self, manga_id: &str, lang: &str, title: &str) -> DomainResult<()> {
        let key = crate::id::AltTitleId::new().to_string();
        sqlx::query(
            "INSERT INTO alt_titles (key, manga_key, language, title) VALUES ($1, $2, $3, $4)",
        )
        .bind(&key)
        .bind(manga_id)
        .bind(lang)
        .bind(title)
        .execute(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn upsert_link(&self, manga_id: &str, provider: &str, url: &str) -> DomainResult<()> {
        let key = crate::id::LinkId::new().to_string();
        sqlx::query(
            "INSERT INTO links (key, manga_key, link_provider, link_url) VALUES ($1, $2, $3, $4)",
        )
        .bind(&key)
        .bind(manga_id)
        .bind(provider)
        .bind(url)
        .execute(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn merge(&self, source_id: &str, target_id: &str) -> DomainResult<()> {
        // Move unique connector mappings from source to target
        sqlx::query(
            r#"UPDATE manga_connector_to_manga SET obj_id = $1 WHERE obj_id = $2 AND manga_connector_name NOT IN (
                SELECT manga_connector_name FROM manga_connector_to_manga WHERE obj_id = $1
            )"#
        )
        .bind(target_id)
        .bind(source_id)
        .execute(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        // Remove any remaining source connector mappings
        sqlx::query("DELETE FROM manga_connector_to_manga WHERE obj_id = $1")
            .bind(source_id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;

        // Move chapters
        sqlx::query("UPDATE chapters SET parent_manga_id = $1 WHERE parent_manga_id = $2")
            .bind(target_id)
            .bind(source_id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;

        // Delete source manga
        sqlx::query("DELETE FROM mangas WHERE key = $1")
            .bind(source_id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;

        Ok(())
    }

    async fn by_tag(&self, tag: &str) -> DomainResult<Vec<Manga>> {
        let rows = sqlx::query_as::<_, MangaRow>(
            r#"SELECT m.key, m.name, m.description, m.cover_url, m.release_status, m.library_id, m.ignore_chapters_before, m.year, m.original_language
               FROM mangas m
               INNER JOIN manga_tag_associations mt ON mt.manga_key = m.key
               WHERE mt.tag = $1
               ORDER BY m.name"#
        )
        .bind(tag)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            result.push(self.build_manga(row).await?);
        }
        Ok(result)
    }

    async fn by_author(&self, author_id: &str) -> DomainResult<Vec<Manga>> {
        let rows = sqlx::query_as::<_, MangaRow>(
            r#"SELECT m.key, m.name, m.description, m.cover_url, m.release_status, m.library_id, m.ignore_chapters_before, m.year, m.original_language
               FROM mangas m
               INNER JOIN manga_authors ma ON ma.manga_key = m.key
               WHERE ma.author_key = $1
               ORDER BY m.name"#
        )
        .bind(author_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            result.push(self.build_manga(row).await?);
        }
        Ok(result)
    }

    async fn recheck_chapters_for_manga(&self, manga_id: Option<&str>) -> DomainResult<i64> {
        let result = match manga_id {
            Some(id) => sqlx::query(
                "DELETE FROM manga_connector_to_chapter WHERE obj_id IN (SELECT key FROM chapters WHERE parent_manga_id = $1) AND (SELECT downloaded FROM chapters WHERE key = manga_connector_to_chapter.obj_id) = 1"
            )
            .bind(id)
            .execute(&self.pool)
            .await,
            None => sqlx::query(
                "DELETE FROM manga_connector_to_chapter WHERE obj_id IN (SELECT key FROM chapters WHERE downloaded = 1)"
            )
            .execute(&self.pool)
            .await,
        }
        .map_err(Self::map_sqlx_error)?;

        Ok(result.rows_affected() as i64)
    }
}

// ── Row types ──

#[derive(Debug, sqlx::FromRow)]
struct MangaRow {
    key: String,
    name: String,
    description: String,
    cover_url: String,
    release_status: String,
    library_id: Option<String>,
    ignore_chapters_before: f64,
    year: Option<i32>,
    original_language: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct ConnectorMappingRow {
    key: String,
    manga_connector_name: String,
    id_on_connector_site: String,
    website_url: Option<String>,
    use_for_download: i32,
}

#[derive(Debug, sqlx::FromRow)]
struct AuthorRow {
    key: String,
    author_name: String,
}

#[derive(Debug, sqlx::FromRow)]
struct TagRow {
    tag: String,
}

#[derive(Debug, sqlx::FromRow)]
struct LinkRow {
    link_provider: String,
    link_url: String,
}

#[derive(Debug, sqlx::FromRow)]
struct AltTitleRow {
    language: String,
    title: String,
}
