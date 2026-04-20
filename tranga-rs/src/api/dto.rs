use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinimalManga {
    pub id: String,
    pub name: String,
    pub description: String,
    pub release_status: String,
    pub connectors: Vec<ConnectorMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MangaDetail {
    pub id: String,
    pub name: String,
    pub description: String,
    pub cover_url: String,
    pub release_status: String,
    pub connectors: Vec<ConnectorMapping>,
    pub ignore_chapters_before: f64,
    pub year: Option<u32>,
    pub original_language: Option<String>,
    pub authors: Vec<AuthorDto>,
    pub tags: Vec<String>,
    pub links: Vec<LinkDto>,
    pub alt_titles: Vec<AltTitleDto>,
    pub file_library_id: Option<String>,
    pub cover_file_name_in_cache: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterDto {
    pub id: String,
    pub manga_id: String,
    pub volume: Option<i32>,
    pub chapter_number: String,
    pub title: Option<String>,
    pub connectors: Vec<ConnectorMapping>,
    pub downloaded: bool,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorMapping {
    pub id: String,
    pub manga_connector_name: String,
    pub entity_id: String,
    pub id_on_connector_site: String,
    pub website_url: Option<String>,
    pub use_for_download: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorDto {
    pub id: String,
    pub author_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkDto {
    pub id: String,
    pub link_provider: String,
    pub link_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AltTitleDto {
    pub language: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileLibraryDto {
    pub id: String,
    pub base_path: String,
    pub library_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryConnectorDto {
    pub id: String,
    pub library_type: String,
    pub base_url: String,
    pub auth: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationConnectorDto {
    pub name: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub http_method: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorInfo {
    pub name: String,
    pub supported_languages: Vec<String>,
    pub icon_url: String,
    pub base_uris: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedResponse<T> {
    pub data: Vec<T>,
    pub page: i32,
    pub total_pages: i32,
    pub total_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerInfo {
    pub id: String,
    pub state: crate::workers::WorkerState,
    pub worker_type: String,
    pub depends_on: Vec<String>,
    pub is_periodic: Option<bool>,
    pub interval_secs: Option<u64>,
    pub last_execution: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRecordDto {
    pub id: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsDto {
    pub number_manga: i64,
    pub number_chapters: i64,
    pub missing_chapters: i64,
    pub downloaded_chapters: i64,
    pub sent_notifications: i64,
    pub actions_taken: i64,
    pub number_authors: i64,
    pub number_tags: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataEntryDto {
    pub manga_id: String,
    pub metadata_fetcher_name: String,
    pub identifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataSearchResultDto {
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub description: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterFilter {
    pub downloaded: Option<bool>,
    pub name: Option<String>,
    pub volume_number: Option<i32>,
    pub chapter_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLibrary {
    pub base_path: String,
    pub library_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToggleDownload {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddToLibrary {
    pub library_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLibraryConnector {
    pub library_type: String,
    pub url: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNotificationConnector {
    pub connector_type: String,
    pub name: String,
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub app_token: Option<String>,
    pub http_method: Option<String>,
    pub body: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub priority: Option<i32>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub topic: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionsFilter {
    pub start: Option<String>,
    pub end: Option<String>,
    pub manga_id: Option<String>,
    pub chapter_id: Option<String>,
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRefreshConfig {
    pub setting: String,
    pub refresh_while_downloading_every_minutes: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_manga_serializes_camel_case() {
        let m = MinimalManga {
            id: "manga_abc".into(),
            name: "Test".into(),
            description: "desc".into(),
            release_status: "Continuing".into(),
            connectors: vec![],
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("releaseStatus"), "{json}");
        assert!(json.contains("connectors"), "{json}");
    }

    #[test]
    fn paged_response_calculates_correctly() {
        let data: Vec<i32> = (0..25).collect();
        let page: Vec<i32> = data.iter().skip(10).take(10).copied().collect();
        let resp = PagedResponse {
            data: page,
            page: 2,
            total_pages: 3,
            total_count: 25,
        };
        assert_eq!(resp.data.len(), 10);
        assert_eq!(resp.page, 2);
        assert_eq!(resp.total_pages, 3);
        assert_eq!(resp.total_count, 25);
    }

    #[test]
    fn chapter_filter_default_is_all_none() {
        let f = ChapterFilter::default();
        assert!(f.downloaded.is_none());
        assert!(f.name.is_none());
        assert!(f.volume_number.is_none());
        assert!(f.chapter_number.is_none());
    }

    #[test]
    fn create_library_deserializes() {
        let json = r#"{"basePath":"/manga","libraryName":"My Library"}"#;
        let cl: CreateLibrary = serde_json::from_str(json).unwrap();
        assert_eq!(cl.base_path, "/manga");
        assert_eq!(cl.library_name, "My Library");
    }

    #[test]
    fn create_library_connector_deserializes() {
        let json = r#"{"libraryType":"Komga","url":"http://localhost:8080","apiKey":"abc"}"#;
        let clc: CreateLibraryConnector = serde_json::from_str(json).unwrap();
        assert_eq!(clc.library_type, "Komga");
    }

    #[test]
    fn create_notification_connector_webhook() {
        let json = r#"{"connectorType":"webhook","name":"myhook","url":"http://hook","httpMethod":"POST","body":"{}","headers":{}}"#;
        let cn: CreateNotificationConnector = serde_json::from_str(json).unwrap();
        assert_eq!(cn.connector_type, "webhook");
        assert_eq!(cn.name, "myhook");
    }

    #[test]
    fn stats_dto_serializes() {
        let s = StatsDto {
            number_manga: 10,
            number_chapters: 100,
            missing_chapters: 20,
            downloaded_chapters: 80,
            sent_notifications: 5,
            actions_taken: 50,
            number_authors: 3,
            number_tags: 15,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("numberManga"), "{json}");
        assert!(json.contains("downloadedChapters"), "{json}");
    }

    #[test]
    fn library_refresh_config_deserializes() {
        let json = r#"{"setting":"AfterEveryChapter","refreshWhileDownloadingEveryMinutes":5}"#;
        let lr: LibraryRefreshConfig = serde_json::from_str(json).unwrap();
        assert_eq!(lr.setting, "AfterEveryChapter");
        assert_eq!(lr.refresh_while_downloading_every_minutes, Some(5));
    }
}
