use sqlx::sqlite::SqlitePool;
use crate::error::AppResult;
use crate::api::dto::*;

pub struct MangaRepo;

impl MangaRepo {
    pub async fn list(pool: &SqlitePool) -> AppResult<Vec<MinimalManga>> {
        let rows = sqlx::query_as::<_, MangaRow>(
            "SELECT key, name, description, cover_url, release_status, library_id, ignore_chapters_before, directory_name, cover_file_name_in_cache, year, original_language FROM mangas ORDER BY name"
        )
        .fetch_all(pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            let connector_ids = Self::get_connector_ids_for_manga(pool, &row.key).await?;
            result.push(MinimalManga {
                id: row.key,
                name: row.name,
                description: row.description,
                release_status: row.release_status,
                connectors: connector_ids,
            });
        }
        Ok(result)
    }

    pub async fn list_downloading(pool: &SqlitePool) -> AppResult<Vec<MinimalManga>> {
        let rows = sqlx::query_as::<_, MangaRow>(
            r#"SELECT m.key, m.name, m.description, m.cover_url, m.release_status, m.library_id, m.ignore_chapters_before, m.directory_name, m.cover_file_name_in_cache, m.year, m.original_language
               FROM mangas m
               WHERE EXISTS (
                   SELECT 1 FROM manga_connector_to_manga mc WHERE mc.obj_id = m.key AND mc.use_for_download = 1
               )
               ORDER BY m.name"#
        )
        .fetch_all(pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            let connector_ids = Self::get_connector_ids_for_manga(pool, &row.key).await?;
            result.push(MinimalManga {
                id: row.key,
                name: row.name,
                description: row.description,
                release_status: row.release_status,
                connectors: connector_ids,
            });
        }
        Ok(result)
    }

    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> AppResult<Option<MangaDetail>> {
        let row = sqlx::query_as::<_, MangaRow>(
            "SELECT key, name, description, cover_url, release_status, library_id, ignore_chapters_before, directory_name, cover_file_name_in_cache, year, original_language FROM mangas WHERE key = $1"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        let row = match row {
            Some(r) => r,
            None => return Ok(None),
        };

        let connector_ids = Self::get_connector_ids_for_manga(pool, &row.key).await?;
        let authors = Self::get_authors(pool, &row.key).await?;
        let tags = Self::get_tags(pool, &row.key).await?;
        let links = Self::get_links(pool, &row.key).await?;
        let alt_titles = Self::get_alt_titles(pool, &row.key).await?;

        Ok(Some(MangaDetail {
            id: row.key,
            name: row.name,
            description: row.description,
            cover_url: row.cover_url,
            release_status: row.release_status,
            connectors: connector_ids,
            ignore_chapters_before: row.ignore_chapters_before,
            year: row.year.map(|y| y as u32),
            original_language: row.original_language,
            authors,
            tags,
            links,
            alt_titles,
            file_library_id: row.library_id,
            cover_file_name_in_cache: row.cover_file_name_in_cache,
        }))
    }

    pub async fn delete(pool: &SqlitePool, id: &str) -> AppResult<bool> {
        let result = sqlx::query("DELETE FROM mangas WHERE key = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_library_id(pool: &SqlitePool, manga_id: &str, library_id: &str) -> AppResult<bool> {
        let result = sqlx::query("UPDATE mangas SET library_id = $1 WHERE key = $2")
            .bind(library_id)
            .bind(manga_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_cover_file_name(pool: &SqlitePool, manga_id: &str, file_name: &str) -> AppResult<bool> {
        let result = sqlx::query("UPDATE mangas SET cover_file_name_in_cache = $1 WHERE key = $2")
            .bind(file_name)
            .bind(manga_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_manga_field(pool: &SqlitePool, manga_id: &str, field: &str, value: &str) -> AppResult<bool> {
        let allowed = ["name", "description", "cover_url", "release_status", "original_language"];
        if !allowed.contains(&field) {
            return Ok(false);
        }
        let sql = format!("UPDATE mangas SET {field} = $1 WHERE key = $2");
        let result = sqlx::query(&sql)
            .bind(value)
            .bind(manga_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn find_manga_like(pool: &SqlitePool, name: &str) -> AppResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT key FROM mangas WHERE name = $1 LIMIT 1"
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn add_connector_mapping(
        pool: &SqlitePool,
        key: &str,
        manga_id: &str,
        connector_name: &str,
        id_on_connector_site: &str,
        website_url: Option<&str>,
        use_for_download: bool,
    ) -> AppResult<ConnectorMapping> {
        sqlx::query(
            r#"INSERT INTO manga_connector_to_manga (key, obj_id, manga_connector_name, id_on_connector_site, website_url, use_for_download)
               VALUES ($1, $2, $3, $4, $5, $6)"#
        )
        .bind(key)
        .bind(manga_id)
        .bind(connector_name)
        .bind(id_on_connector_site)
        .bind(website_url)
        .bind(use_for_download)
        .execute(pool)
        .await?;

        Ok(ConnectorMapping {
            id: key.to_string(),
            manga_connector_name: connector_name.to_string(),
            entity_id: manga_id.to_string(),
            id_on_connector_site: id_on_connector_site.to_string(),
            website_url: website_url.map(|s| s.to_string()),
            use_for_download,
        })
    }

    pub async fn set_connector_use_for_download(pool: &SqlitePool, manga_id: &str, connector_name: &str, value: bool) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE manga_connector_to_manga SET use_for_download = $1 WHERE obj_id = $2 AND manga_connector_name = $3"
        )
        .bind(value)
        .bind(manga_id)
        .bind(connector_name)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_connectors_for_download(pool: &SqlitePool) -> AppResult<Vec<ConnectorMapping>> {
        let rows = sqlx::query_as::<_, ConnectorMappingRow>(
            "SELECT mct.key, mct.manga_connector_name, mct.obj_id, mct.id_on_connector_site, mct.website_url, mct.use_for_download
            FROM manga_connector_to_manga mct
            JOIN mangas m ON mct.obj_id = m.key
            WHERE mct.use_for_download = 1 AND m.library_id IS NOT NULL"
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| ConnectorMapping {
            id: r.key,
            manga_connector_name: r.manga_connector_name,
            entity_id: r.entity_id,
            id_on_connector_site: r.id_on_connector_site,
            website_url: r.website_url,
            use_for_download: r.use_for_download != 0,
        }).collect())
    }

    pub async fn insert_author_if_new(pool: &SqlitePool, key: &str, name: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO authors (key, author_name) VALUES ($1, $2) ON CONFLICT (key) DO NOTHING"
        )
        .bind(key)
        .bind(name)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn link_author(pool: &SqlitePool, manga_key: &str, author_key: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO manga_authors (manga_key, author_key) VALUES ($1, $2) ON CONFLICT DO NOTHING"
        )
        .bind(manga_key)
        .bind(author_key)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn insert_tag_if_new(pool: &SqlitePool, tag: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO manga_tags (tag) VALUES ($1) ON CONFLICT (tag) DO NOTHING"
        )
        .bind(tag)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn link_tag(pool: &SqlitePool, manga_key: &str, tag: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO manga_tag_associations (manga_key, tag) VALUES ($1, $2) ON CONFLICT DO NOTHING"
        )
        .bind(manga_key)
        .bind(tag)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn insert_link(pool: &SqlitePool, key: &str, manga_key: &str, provider: &str, url: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO links (key, manga_key, link_provider, link_url) VALUES ($1, $2, $3, $4)"
        )
        .bind(key)
        .bind(manga_key)
        .bind(provider)
        .bind(url)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn insert_alt_title(pool: &SqlitePool, key: &str, manga_key: &str, language: &str, title: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO alt_titles (key, manga_key, language, title) VALUES ($1, $2, $3, $4)"
        )
        .bind(key)
        .bind(manga_key)
        .bind(language)
        .bind(title)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete_no_download_manga(pool: &SqlitePool) -> AppResult<u64> {
        let result = sqlx::query(
            r#"DELETE FROM mangas WHERE key NOT IN (
                SELECT DISTINCT obj_id FROM manga_connector_to_manga WHERE use_for_download = 1
            )"#
        )
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn get_name(pool: &SqlitePool, manga_id: &str) -> AppResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM mangas WHERE key = $1"
        )
        .bind(manga_id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn list_all_names(pool: &SqlitePool) -> AppResult<Vec<(String, String)>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT key, name FROM mangas ORDER BY name"
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn toggle_connector_download(pool: &SqlitePool, manga_id: &str, connector_name: &str) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE manga_connector_to_manga
               SET use_for_download = CASE WHEN use_for_download = 0 THEN 1 ELSE 0 END
               WHERE obj_id = $1 AND manga_connector_name = $2"#
        )
        .bind(manga_id)
        .bind(connector_name)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_connector_download_status(pool: &SqlitePool, manga_id: &str, connector_name: &str) -> AppResult<Option<bool>> {
        let row = sqlx::query(
            "SELECT use_for_download FROM manga_connector_to_manga WHERE obj_id = $1 AND manga_connector_name = $2"
        )
        .bind(manga_id)
        .bind(connector_name)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|r| {
            let val: i32 = sqlx::Row::get(&r, "use_for_download");
            val != 0
        }))
    }

    pub async fn set_connector_download(pool: &SqlitePool, manga_id: &str, connector_name: &str, enabled: bool) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE manga_connector_to_manga SET use_for_download = $3 WHERE obj_id = $1 AND manga_connector_name = $2"
        )
        .bind(manga_id)
        .bind(connector_name)
        .bind(if enabled { 1 } else { 0 })
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_connector_mapping(pool: &SqlitePool, manga_id: &str, connector_name: &str) -> AppResult<Option<ConnectorMapping>> {
        let row = sqlx::query_as::<_, ConnectorMappingRow>(
            "SELECT key, manga_connector_name, obj_id, id_on_connector_site, website_url, use_for_download FROM manga_connector_to_manga WHERE obj_id = $1 AND manga_connector_name = $2"
        )
        .bind(manga_id)
        .bind(connector_name)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| ConnectorMapping {
            id: r.key,
            manga_connector_name: r.manga_connector_name,
            entity_id: r.entity_id,
            id_on_connector_site: r.id_on_connector_site,
            website_url: r.website_url,
            use_for_download: r.use_for_download != 0,
        }))
    }

    pub async fn merge_manga(pool: &SqlitePool, source_id: &str, target_id: &str) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE manga_connector_to_manga SET obj_id = $1 WHERE obj_id = $2 AND manga_connector_name NOT IN (
                SELECT manga_connector_name FROM manga_connector_to_manga WHERE obj_id = $1
            )"#
        )
        .bind(target_id)
        .bind(source_id)
        .execute(pool)
        .await?;

        sqlx::query("DELETE FROM manga_connector_to_manga WHERE obj_id = $1")
            .bind(source_id)
            .execute(pool)
            .await?;

        sqlx::query("UPDATE chapters SET parent_manga_id = $1 WHERE parent_manga_id = $2")
            .bind(target_id)
            .bind(source_id)
            .execute(pool)
            .await?;

        sqlx::query(
            r#"UPDATE manga_connector_to_chapter SET obj_id = (
                SELECT c.key FROM chapters c WHERE c.parent_manga_id = $1
            ) WHERE obj_id IN (
                SELECT mc2.obj_id FROM manga_connector_to_chapter mc2
                INNER JOIN chapters ch2 ON ch2.key = mc2.obj_id
                WHERE ch2.parent_manga_id = $2
            )"#
        )
        .bind(target_id)
        .bind(source_id)
        .execute(pool)
        .await?;

        Self::delete(pool, source_id).await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        pool: &SqlitePool,
        key: &str,
        name: &str,
        description: &str,
        cover_url: &str,
        release_status: &str,
        library_id: Option<&str>,
        directory_name: &str,
        year: Option<i32>,
        original_language: Option<&str>,
    ) -> AppResult<()> {
        sqlx::query(
            r#"INSERT INTO mangas (key, name, description, cover_url, release_status, library_id, directory_name, year, original_language)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
        )
        .bind(key)
        .bind(name)
        .bind(description)
        .bind(cover_url)
        .bind(release_status)
        .bind(library_id)
        .bind(directory_name)
        .bind(year)
        .bind(original_language)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_by_tag(pool: &SqlitePool, tag: &str) -> AppResult<Vec<MinimalManga>> {
        let rows = sqlx::query_as::<_, MangaRow>(
            r#"SELECT m.key, m.name, m.description, m.cover_url, m.release_status, m.library_id, m.ignore_chapters_before, m.directory_name, m.cover_file_name_in_cache, m.year, m.original_language
               FROM mangas m
               INNER JOIN manga_tag_associations mt ON mt.manga_key = m.key
               INNER JOIN manga_tags t ON t.tag = mt.tag
               WHERE t.tag = $1
               ORDER BY m.name"#
        )
        .bind(tag)
        .fetch_all(pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            let connector_ids = Self::get_connector_ids_for_manga(pool, &row.key).await?;
            result.push(MinimalManga {
                id: row.key,
                name: row.name,
                description: row.description,
                release_status: row.release_status,
                connectors: connector_ids,
            });
        }
        Ok(result)
    }

    pub async fn get_by_author(pool: &SqlitePool, author_key: &str) -> AppResult<Vec<MangaDetail>> {
        let rows = sqlx::query_as::<_, MangaRow>(
            r#"SELECT m.key, m.name, m.description, m.cover_url, m.release_status, m.library_id, m.ignore_chapters_before, m.directory_name, m.cover_file_name_in_cache, m.year, m.original_language
               FROM mangas m
               INNER JOIN manga_authors ma ON ma.manga_key = m.key
               WHERE ma.author_key = $1
               ORDER BY m.name"#
        )
        .bind(author_key)
        .fetch_all(pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            let connector_ids = Self::get_connector_ids_for_manga(pool, &row.key).await?;
            let authors = Self::get_authors(pool, &row.key).await?;
            let tags = Self::get_tags(pool, &row.key).await?;
            let links = Self::get_links(pool, &row.key).await?;
            let alt_titles = Self::get_alt_titles(pool, &row.key).await?;
            result.push(MangaDetail {
                id: row.key,
                name: row.name,
                description: row.description,
                cover_url: row.cover_url,
                release_status: row.release_status,
                connectors: connector_ids,
                ignore_chapters_before: row.ignore_chapters_before,
                year: row.year.map(|y| y as u32),
                original_language: row.original_language,
                authors,
                tags,
                links,
                alt_titles,
                file_library_id: row.library_id,
                cover_file_name_in_cache: row.cover_file_name_in_cache,
            });
        }
        Ok(result)
    }

    async fn get_connector_ids_for_manga(pool: &SqlitePool, manga_key: &str) -> AppResult<Vec<ConnectorMapping>> {
        let rows = sqlx::query_as::<_, ConnectorMappingRow>(
            "SELECT key, manga_connector_name, obj_id, id_on_connector_site, website_url, use_for_download FROM manga_connector_to_manga WHERE obj_id = $1"
        )
        .bind(manga_key)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ConnectorMapping {
                id: r.key,
                manga_connector_name: r.manga_connector_name,
                entity_id: r.entity_id,
                id_on_connector_site: r.id_on_connector_site,
                website_url: r.website_url,
                use_for_download: r.use_for_download != 0,
            })
            .collect())
    }

    async fn get_authors(pool: &SqlitePool, manga_key: &str) -> AppResult<Vec<AuthorDto>> {
        let rows = sqlx::query_as::<_, AuthorRow>(
            r#"SELECT a.key, a.author_name
               FROM authors a
               INNER JOIN manga_authors ma ON ma.author_key = a.key
               WHERE ma.manga_key = $1"#
        )
        .bind(manga_key)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| AuthorDto {
                id: r.key,
                author_name: r.author_name,
            })
            .collect())
    }

    async fn get_tags(pool: &SqlitePool, manga_key: &str) -> AppResult<Vec<String>> {
        let rows = sqlx::query_as::<_, TagRow>(
            r#"SELECT t.tag
               FROM manga_tags t
               INNER JOIN manga_tag_associations mt ON mt.tag = t.tag
               WHERE mt.manga_key = $1"#
        )
        .bind(manga_key)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.tag).collect())
    }

    async fn get_links(pool: &SqlitePool, manga_key: &str) -> AppResult<Vec<LinkDto>> {
        let rows = sqlx::query_as::<_, LinkRow>(
            "SELECT key, link_provider, link_url FROM links WHERE manga_key = $1"
        )
        .bind(manga_key)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| LinkDto {
                id: r.key,
                link_provider: r.link_provider,
                link_url: r.link_url,
            })
            .collect())
    }

    async fn get_alt_titles(pool: &SqlitePool, manga_key: &str) -> AppResult<Vec<AltTitleDto>> {
        let rows = sqlx::query_as::<_, AltTitleRow>(
            "SELECT language, title FROM alt_titles WHERE manga_key = $1"
        )
        .bind(manga_key)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| AltTitleDto {
                language: r.language,
                title: r.title,
            })
            .collect())
    }

    pub async fn get_authors_public(pool: &SqlitePool, manga_key: &str) -> AppResult<Vec<AuthorDto>> {
        Self::get_authors(pool, manga_key).await
    }

    pub async fn list_authors(pool: &SqlitePool) -> AppResult<Vec<AuthorDto>> {
        let rows = sqlx::query_as::<_, AuthorRow>(
            "SELECT key, author_name FROM authors ORDER BY author_name"
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| AuthorDto {
            id: r.key,
            author_name: r.author_name,
        }).collect())
    }

    pub async fn get_author_by_id(pool: &SqlitePool, author_id: &str) -> AppResult<Option<AuthorDto>> {
        let row = sqlx::query_as::<_, AuthorRow>(
            "SELECT key, author_name FROM authors WHERE key = $1"
        )
        .bind(author_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| AuthorDto {
            id: r.key,
            author_name: r.author_name,
        }))
    }

    pub async fn get_manga_by_author(pool: &SqlitePool, author_id: &str) -> AppResult<Vec<MinimalManga>> {
        let rows = sqlx::query_as::<_, MangaRow>(
            r#"SELECT m.key, m.name, m.description, m.cover_url, m.release_status, m.library_id, m.ignore_chapters_before, m.directory_name, m.cover_file_name_in_cache, m.year, m.original_language
               FROM mangas m
               INNER JOIN manga_authors ma ON ma.manga_key = m.key
               WHERE ma.author_key = $1
               ORDER BY m.name"#
        )
        .bind(author_id)
        .fetch_all(pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            let connector_ids = Self::get_connector_ids_for_manga(pool, &row.key).await?;
            result.push(MinimalManga {
                id: row.key,
                name: row.name,
                description: row.description,
                release_status: row.release_status,
                connectors: connector_ids,
            });
        }
        Ok(result)
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct MangaRow {
    pub key: String,
    pub name: String,
    pub description: String,
    pub cover_url: String,
    pub release_status: String,
    pub library_id: Option<String>,
    pub ignore_chapters_before: f64,
    pub directory_name: String,
    pub cover_file_name_in_cache: Option<String>,
    pub year: Option<i32>,
    pub original_language: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct ConnectorMappingRow {
    pub key: String,
    pub manga_connector_name: String,
    #[sqlx(rename = "obj_id")]
    pub entity_id: String,
    pub id_on_connector_site: String,
    pub website_url: Option<String>,
    pub use_for_download: i32,
}

#[derive(Debug, sqlx::FromRow)]
pub struct AuthorRow {
    pub key: String,
    pub author_name: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct TagRow {
    pub tag: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct LinkRow {
    pub key: String,
    pub link_provider: String,
    pub link_url: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct AltTitleRow {
    pub language: String,
    pub title: String,
}
