use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub user_agent: String,
    pub image_compression: i32,
    pub black_white_images: bool,
    pub flare_solverr_url: String,
    pub chapter_naming_scheme: String,
    pub work_cycle_timeout_ms: i32,
    pub download_language: String,
    pub max_concurrent_downloads: i32,
    pub max_concurrent_workers: i32,
    pub library_refresh_setting: String,
    pub refresh_library_while_downloading_every_minutes: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            user_agent: String::from("Tranga"),
            image_compression: 40,
            black_white_images: false,
            flare_solverr_url: String::new(),
            chapter_naming_scheme: String::from("%M - ?V(Vol.%V )Ch.%C?T( - %T)"),
            work_cycle_timeout_ms: 20000,
            download_language: String::from("en"),
            max_concurrent_downloads: 0,
            max_concurrent_workers: 0,
            library_refresh_setting: String::from("AfterMangaFinished"),
            refresh_library_while_downloading_every_minutes: 10,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub database_url: String,
    pub download_location: PathBuf,
    pub check_chapters_before_start: bool,
    pub match_exact_chapter_name: bool,
    pub create_comicinfo_xml: bool,
    pub always_include_volume_in_filename: bool,
    pub http_request_timeout_secs: u64,
    pub requests_per_minute: u32,
    pub worker_timeout_secs: u64,
    pub chromium_wait_time_secs: u64,
    pub settings: Arc<RwLock<Settings>>,
    pub working_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> Self {
        let env_dir = std::env::var("WORKING_DIR")
            .map(PathBuf::from)
            .ok();
        
        let working_dir = if let Some(dir) = env_dir {
            dir
        } else if cfg!(debug_assertions) {
            PathBuf::from("./debug")
        } else if std::path::Path::new("/usr/share/tranga-api").is_dir() {
            PathBuf::from("/usr/share/tranga-api")
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        };

        let settings_path = working_dir.join("settings.json");
        let settings = if settings_path.exists() {
            let data = std::fs::read_to_string(&settings_path)
                .expect("Failed to read settings.json");
            serde_json::from_str(&data).unwrap_or_else(|e| {
                tracing::warn!("Failed to parse settings.json, using defaults: {e}");
                Settings::default()
            })
        } else {
            Settings::default()
        };

        let port: u16 = std::env::var("PORT")
            .unwrap_or_else(|_| "6531".into())
            .parse()
            .unwrap_or(6531);

        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| format!("sqlite://{}", working_dir.join("tranga.db").display()));

        let download_location = PathBuf::from(
            std::env::var("DOWNLOAD_LOCATION").unwrap_or_else(|_| "/Manga".into()),
        );

        let check_chapters_before_start = std::env::var("CHECK_CHAPTERS_BEFORE_START")
            .unwrap_or_else(|_| "true".into())
            .parse()
            .unwrap_or(true);

        let match_exact_chapter_name = std::env::var("MATCH_EXACT_CHAPTER_NAME")
            .unwrap_or_else(|_| "true".into())
            .parse()
            .unwrap_or(true);

        let create_comicinfo_xml = std::env::var("CREATE_COMICINFO_XML")
            .unwrap_or_else(|_| "true".into())
            .parse()
            .unwrap_or(true);

        let always_include_volume_in_filename = std::env::var("ALWAYS_INCLUDE_VOLUME_IN_FILENAME")
            .unwrap_or_else(|_| "false".into())
            .parse()
            .unwrap_or(false);

        let http_request_timeout_secs = std::env::var("HTTP_REQUEST_TIMEOUT")
            .unwrap_or_else(|_| "60".into())
            .parse()
            .unwrap_or(60);

        let requests_per_minute: u32 = std::env::var("REQUESTS_PER_MINUTE")
            .unwrap_or_else(|_| "90".into())
            .parse()
            .unwrap_or(90);

        let worker_timeout_secs = std::env::var("WORKER_TIMEOUT")
            .unwrap_or_else(|_| "600".into())
            .parse()
            .unwrap_or(600);

        let chromium_wait_time_secs = std::env::var("CHROMIUM_WAIT_TIME")
            .unwrap_or_else(|_| "15".into())
            .parse()
            .unwrap_or(15);

        info!("Config loaded: port={port}, database_url={database_url}");

        Self {
            port,
            database_url,
            download_location,
            check_chapters_before_start,
            match_exact_chapter_name,
            create_comicinfo_xml,
            always_include_volume_in_filename,
            http_request_timeout_secs,
            requests_per_minute,
            worker_timeout_secs,
            chromium_wait_time_secs,
            settings: Arc::new(RwLock::new(settings)),
            working_dir,
        }
    }

    pub fn is_sqlite(&self) -> bool {
        self.database_url.starts_with("sqlite")
    }

    pub fn image_cache_dir(&self, size: &str) -> PathBuf {
        self.working_dir.join("imageCache").join(size)
    }

    pub fn cover_image_cache_original(&self) -> PathBuf {
        self.image_cache_dir("original")
    }

    pub fn cover_image_cache_large(&self) -> PathBuf {
        self.image_cache_dir("large")
    }

    pub fn cover_image_cache_medium(&self) -> PathBuf {
        self.image_cache_dir("medium")
    }

    pub fn cover_image_cache_small(&self) -> PathBuf {
        self.image_cache_dir("small")
    }

    pub async fn persist_settings(&self) {
        let settings = self.settings.read().await;
        let path = self.working_dir.join("settings.json");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&*settings) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::error!("Failed to persist settings: {e}");
                }
            }
            Err(e) => tracing::error!("Failed to serialize settings: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_values() {
        let s = Settings::default();
        assert_eq!(s.user_agent, "Tranga");
        assert_eq!(s.image_compression, 40);
        assert!(!s.black_white_images);
        assert!(s.flare_solverr_url.is_empty());
        assert_eq!(s.download_language, "en");
        assert_eq!(s.library_refresh_setting, "AfterMangaFinished");
        assert_eq!(s.max_concurrent_downloads, 0);
        assert_eq!(s.max_concurrent_workers, 0);
    }

    #[test]
    fn settings_serde_roundtrip() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).expect("serialize");
        let parsed: Settings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.user_agent, s.user_agent);
        assert_eq!(parsed.image_compression, s.image_compression);
        assert_eq!(parsed.download_language, s.download_language);
    }

    #[test]
    fn settings_json_uses_camel_case() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).expect("serialize");
        assert!(json.contains("userAgent"), "should use camelCase: {json}");
        assert!(json.contains("imageCompression"), "should use camelCase: {json}");
        assert!(json.contains("blackWhiteImages"), "should use camelCase: {json}");
        assert!(json.contains("flareSolverrUrl"), "should use camelCase: {json}");
        assert!(json.contains("chapterNamingScheme"), "should use camelCase: {json}");
        assert!(json.contains("downloadLanguage"), "should use camelCase: {json}");
        assert!(json.contains("libraryRefreshSetting"), "should use camelCase: {json}");
    }

    #[test]
    fn is_sqlite_detects_sqlite_url() {
        let config = Config {
            database_url: "sqlite:///tmp/test.db".into(),
            ..test_config()
        };
        assert!(config.is_sqlite());
    }

    #[test]
    fn is_sqlite_rejects_postgres_url() {
        let config = Config {
            database_url: "postgres://user:pass@host/db".into(),
            ..test_config()
        };
        assert!(!config.is_sqlite());
    }

    #[test]
    fn image_cache_dir_constructs_correct_path() {
        let config = test_config();
        assert_eq!(
            config.image_cache_dir("original"),
            config.working_dir.join("imageCache").join("original")
        );
        assert_eq!(
            config.cover_image_cache_large(),
            config.working_dir.join("imageCache").join("large")
        );
        assert_eq!(
            config.cover_image_cache_small(),
            config.working_dir.join("imageCache").join("small")
        );
    }

    fn test_config() -> Config {
        Config {
            port: 6531,
            database_url: "sqlite:///tmp/test.db".into(),
            download_location: PathBuf::from("/Manga"),
            check_chapters_before_start: true,
            match_exact_chapter_name: true,
            create_comicinfo_xml: true,
            always_include_volume_in_filename: false,
            http_request_timeout_secs: 60,
            requests_per_minute: 90,
            worker_timeout_secs: 600,
            chromium_wait_time_secs: 15,
            settings: Arc::new(RwLock::new(Settings::default())),
            working_dir: PathBuf::from("/tmp/tranga-test"),
        }
    }
}
