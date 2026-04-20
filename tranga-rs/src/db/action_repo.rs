use sqlx::sqlite::SqlitePool;
use crate::error::AppResult;
use crate::api::dto::*;

pub struct ActionRepo;

impl ActionRepo {
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        pool: &SqlitePool,
        key: &str,
        action: &str,
        manga_id: Option<&str>,
        chapter_id: Option<&str>,
        filename: Option<&str>,
        from_path: Option<&str>,
        to_path: Option<&str>,
    ) -> AppResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            r#"INSERT INTO actions (key, action, performed_at, manga_id, chapter_id, filename, from_path, to_path)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#
        )
        .bind(key)
        .bind(action)
        .bind(now)
        .bind(manga_id)
        .bind(chapter_id)
        .bind(filename)
        .bind(from_path)
        .bind(to_path)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn list_types() -> Vec<String> {
        vec![
            "Startup".into(),
            "ChapterDownloaded".into(),
            "ChaptersRetrieved".into(),
            "CoverDownloaded".into(),
            "DataMoved".into(),
            "LibraryMoved".into(),
            "MetadataUpdated".into(),
        ]
    }

    pub async fn filter(
        pool: &SqlitePool,
        filter: &ActionsFilter,
        page: i32,
        per_page: i32,
    ) -> AppResult<PagedResponse<ActionRecordDto>> {
        let mut sql = String::from("SELECT key, action, performed_at, manga_id, chapter_id, filename, from_path, to_path, metadata_fetcher, file_library_id FROM actions WHERE 1=1");
        let mut param_idx = 1i32;
        let mut binds: Vec<String> = Vec::new();

        if let Some(ref start) = filter.start {
            sql.push_str(&format!(" AND performed_at >= ${param_idx}"));
            binds.push(start.clone());
            param_idx += 1;
        }
        if let Some(ref end) = filter.end {
            sql.push_str(&format!(" AND performed_at <= ${param_idx}"));
            binds.push(end.clone());
            param_idx += 1;
        }
        if let Some(ref manga_id) = filter.manga_id {
            sql.push_str(&format!(" AND manga_id = ${param_idx}"));
            binds.push(manga_id.clone());
            param_idx += 1;
        }
        if let Some(ref chapter_id) = filter.chapter_id {
            sql.push_str(&format!(" AND chapter_id = ${param_idx}"));
            binds.push(chapter_id.clone());
            param_idx += 1;
        }
        if let Some(ref action) = filter.action {
            sql.push_str(&format!(" AND action = ${param_idx}"));
            binds.push(action.clone());
        }

        sql.push_str(" ORDER BY performed_at DESC");

        let mut q = sqlx::query_as::<_, ActionRow>(&sql);
        for val in &binds {
            q = q.bind(val);
        }

        let rows = q.fetch_all(pool).await?;

        let total_count = rows.len() as i32;
        let total_pages = if total_count == 0 { 0 } else { (total_count - 1) / per_page + 1 };
        let skip = ((page - 1) * per_page) as usize;
        let page_data: Vec<ActionRecordDto> = rows
            .into_iter()
            .skip(skip)
            .take(per_page as usize)
            .map(|r| ActionRecordDto {
                id: r.key,
                action: r.action,
                performed_at: r.performed_at,
                manga_id: r.manga_id,
                chapter_id: r.chapter_id,
                filename: r.filename,
                from_path: r.from_path,
                to_path: r.to_path,
                metadata_fetcher: None,
                file_library_id: None,
            })
            .collect();

        Ok(PagedResponse {
            data: page_data,
            page,
            total_pages,
            total_count,
        })
    }

    pub async fn cleanup(pool: &SqlitePool) -> AppResult<()> {
        sqlx::query("DELETE FROM actions")
            .execute(pool)
            .await?;
        Ok(())
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct ActionRow {
    pub key: String,
    pub action: String,
    pub performed_at: String,
    pub manga_id: Option<String>,
    pub chapter_id: Option<String>,
    pub filename: Option<String>,
    pub from_path: Option<String>,
    pub to_path: Option<String>,
    pub metadata_fetcher: Option<String>,
    pub file_library_id: Option<String>,
}
