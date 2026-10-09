//! Startup configuration: defaults, then `kaze.toml`, then `KAZE_*`
//! environment variables (nested keys use `__`, e.g.
//! `KAZE_JOBS__CONCURRENCY=8`).

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};

use crate::{
    app::Settings,
    download::naming::{DEFAULT_TEMPLATE, Template},
    jobs::{Limits, RetryPolicy, scheduler::Schedule},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Address the web server listens on.
    pub listen: SocketAddr,
    /// Where kaze keeps its database.
    pub data_dir: PathBuf,
    /// Library root; each series gets a folder inside.
    pub library_dir: PathBuf,
    /// Chapter file name template.
    pub naming_template: String,
    /// When set, API requests must send it as `X-Api-Key` (or `?apikey=`).
    pub api_key: Option<String>,
    pub jobs: JobsConfig,
    pub http: HttpConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JobsConfig {
    /// Jobs running at once.
    pub concurrency: usize,
    /// Jobs running at once against a single source.
    pub per_source: usize,
    /// Pages of one chapter fetched at once.
    pub page_concurrency: usize,
    pub sync_latest_minutes: u64,
    pub refresh_hours: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    pub user_agent: String,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: ([0, 0, 0, 0], 6531).into(),
            data_dir: "data".into(),
            library_dir: "library".into(),
            naming_template: DEFAULT_TEMPLATE.to_owned(),
            api_key: None,
            jobs: JobsConfig::default(),
            http: HttpConfig::default(),
        }
    }
}

impl Default for JobsConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            per_source: 2,
            page_concurrency: 4,
            sync_latest_minutes: 15,
            refresh_hours: 24,
        }
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            user_agent: kaze_sources::http::DEFAULT_USER_AGENT.to_owned(),
            timeout_secs: 30,
        }
    }
}

impl Config {
    /// Load configuration; a missing `file` is fine (defaults + env).
    pub fn load(file: Option<&std::path::Path>) -> Result<Self, Box<figment::Error>> {
        let mut figment = Figment::from(Serialized::defaults(Self::default()));
        if let Some(file) = file {
            figment = figment.merge(Toml::file(file));
        }
        figment
            .merge(Env::prefixed("KAZE_").split("__"))
            .extract()
            .map_err(Box::new)
    }

    pub fn template(&self) -> Result<Template, crate::download::naming::TemplateError> {
        self.naming_template.parse()
    }

    #[must_use]
    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("kaze.db")
    }

    #[must_use]
    pub fn settings(&self) -> Settings {
        Settings {
            limits: Limits {
                global: self.jobs.concurrency.max(1),
                per_source: self.jobs.per_source.max(1),
            },
            retry: RetryPolicy::default(),
            schedule: Schedule {
                sync_latest_every: Duration::from_secs(self.jobs.sync_latest_minutes.max(1) * 60),
                refresh_after: Duration::from_secs(self.jobs.refresh_hours.max(1) * 3600),
                ..Schedule::default()
            },
        }
    }

    #[must_use]
    pub fn http(&self) -> kaze_sources::http::HttpConfig {
        kaze_sources::http::HttpConfig {
            user_agent: self.http.user_agent.clone(),
            timeout: Duration::from_secs(self.http.timeout_secs),
            ..kaze_sources::http::HttpConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use figment::Jail;

    use super::*;

    #[test]
    #[allow(clippy::result_large_err)] // the closure type is figment::Jail's, not ours
    fn layers_file_and_env() {
        // `Jail` gives the test its own working dir and environment.
        Jail::expect_with(|jail| {
            jail.create_file(
                "kaze.toml",
                r#"
                library_dir = "/manga"
                [jobs]
                concurrency = 8
                "#,
            )?;
            jail.set_env("KAZE_JOBS__PER_SOURCE", "1");
            jail.set_env("KAZE_API_KEY", "secret");

            let config = Config::load(Some("kaze.toml".as_ref())).unwrap();
            assert_eq!(config.library_dir, PathBuf::from("/manga"));
            assert_eq!(config.jobs.concurrency, 8);
            assert_eq!(config.jobs.per_source, 1);
            assert_eq!(config.api_key.as_deref(), Some("secret"));
            assert_eq!(config.jobs.refresh_hours, 24, "untouched defaults remain");
            Ok(())
        });
    }

    #[test]
    #[allow(clippy::result_large_err)]
    fn rejects_unknown_keys() {
        Jail::expect_with(|jail| {
            jail.create_file("kaze.toml", "libary_dir = \"/typo\"")?;
            assert!(Config::load(Some("kaze.toml".as_ref())).is_err());
            Ok(())
        });
    }
}
