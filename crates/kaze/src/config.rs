//! Startup configuration: defaults, then `kaze.toml`, then `KAZE_*`
//! environment variables (nested keys use `__`, e.g.
//! `KAZE_SETTINGS__CONCURRENCY=8`).
//!
//! Only what is needed to boot lives at the top level. Anything under
//! `[settings]` overrides — and locks — the matching UI setting (see
//! `crate::settings`).

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Address the web server listens on.
    pub listen: SocketAddr,
    /// Where kaze keeps its database.
    pub data_dir: PathBuf,
    /// Library root; each series gets a folder inside.
    pub library_dir: PathBuf,
    pub http: HttpConfig,
    /// Locked settings; validated by `SettingsStore::load`.
    pub settings: Map<String, Value>,
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
            http: HttpConfig::default(),
            settings: Map::new(),
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

/// `KAZE_*` variables read elsewhere (the CLI and the log setup), which the
/// strict loader would otherwise reject as unknown settings.
const NOT_CONFIG: &[&str] = &["CONFIG", "LOG", "LOG_FORMAT"];

impl Config {
    /// Load configuration; without a file it is defaults plus env.
    pub fn load(file: Option<&std::path::Path>) -> Result<Self, Box<figment::Error>> {
        let mut figment = Figment::from(Serialized::defaults(Self::default()));
        if let Some(file) = file {
            figment = figment.merge(Toml::file(file));
        }
        figment
            .merge(Env::prefixed("KAZE_").ignore(NOT_CONFIG).split("__"))
            .extract()
            .map_err(Box::new)
    }

    #[must_use]
    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("kaze.db")
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
                [settings]
                concurrency = 8
                "#,
            )?;
            jail.set_env("KAZE_SETTINGS__PER_SOURCE", "1");
            jail.set_env("KAZE_HTTP__TIMEOUT_SECS", "5");
            jail.set_env("KAZE_LOG", "kaze=debug");
            jail.set_env("KAZE_LOG_FORMAT", "json");
            jail.set_env("KAZE_CONFIG", "kaze.toml");

            let config = Config::load(Some("kaze.toml".as_ref())).unwrap();
            assert_eq!(config.library_dir, PathBuf::from("/manga"));
            assert_eq!(config.http.timeout_secs, 5);
            assert_eq!(config.settings["concurrency"], 8);
            assert_eq!(config.settings["per_source"], 1);
            assert_eq!(
                config.data_dir,
                PathBuf::from("data"),
                "untouched defaults remain"
            );
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
