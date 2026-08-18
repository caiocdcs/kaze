#[cfg(feature = "chromium")]
use std::sync::Arc as StdArc;
#[cfg(feature = "chromium")]
use std::time::Duration;

#[cfg(feature = "chromium")]
use anyhow::Context;
#[cfg(feature = "chromium")]
use headless_chrome::{Browser, LaunchOptions};
#[cfg(feature = "chromium")]
use tracing::{debug, info};

#[cfg(feature = "chromium")]
pub struct BrowserManager {
    browser: StdArc<tokio::sync::Mutex<Option<Browser>>>,
    fetch_sem: StdArc<tokio::sync::Semaphore>,
    wait_time: Duration,
}

#[cfg(feature = "chromium")]
impl BrowserManager {
    pub fn new(wait_time_secs: u64) -> Self {
        Self {
            browser: StdArc::new(tokio::sync::Mutex::new(None)),
            fetch_sem: StdArc::new(tokio::sync::Semaphore::new(1)),
            wait_time: Duration::from_secs(wait_time_secs),
        }
    }

    async fn get_or_create(&self) -> Result<Browser, anyhow::Error> {
        let mut guard = self.browser.lock().await;
        if guard.is_none() {
            info!("Launching headless Chromium (first use)");
            let sandbox = std::env::var("CHROMIUM_NO_SANDBOX")
                .map(|v| !matches!(v.as_str(), "true" | "1" | "yes"))
                .unwrap_or(true);
            
            let chrome_path = std::env::var("CHROMIUM_PATH")
                .ok()
                .or_else(|| {
                    let paths = [
                        "/usr/bin/chromium",
                        "/usr/bin/chromium-browser",
                        "/usr/bin/google-chrome",
                        "/usr/bin/google-chrome-stable",
                        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                    ];
                    paths.iter().find(|p| std::path::Path::new(p).exists()).map(|s| s.to_string())
                });
            
            let browser = tokio::task::spawn_blocking(move || {
                let mut options = LaunchOptions {
                    headless: true,
                    sandbox,
                    window_size: Some((1920, 1080)),
                    args: vec![
                        std::ffi::OsStr::new("--disable-dev-shm-usage"),
                        std::ffi::OsStr::new("--disable-gpu"),
                    ],
                    ..Default::default()
                };
                
                if let Some(path) = chrome_path {
                    info!("Using Chromium at: {}", path);
                    options.path = Some(std::path::PathBuf::from(path));
                }
                
                Browser::new(options)
            })
            .await??;
            *guard = Some(browser);
        }
        Ok(guard.as_ref().unwrap().clone())
    }

    pub async fn fetch_html(&self, url: &str) -> Result<String, anyhow::Error> {
        debug!("Fetching HTML via headless Chromium: {url}");
        let _permit = self.fetch_sem.acquire().await?;
        let browser = self.get_or_create().await?;
        let wait = self.wait_time;
        let url_owned = url.to_string();

        let raw = tokio::task::spawn_blocking(move || -> Result<String, anyhow::Error> {
            let tab = browser.new_tab().context("new_tab failed")?;
            tab.navigate_to(&url_owned).context("navigate_to failed")?;
            tab.wait_until_navigated()
                .context("wait_until_navigated failed")?;
            std::thread::sleep(wait);
            let html = tab.get_content().context("get_content failed")?;
            let _ = tab.close(true);
            debug!("Got HTML from Chromium: {} bytes", html.len());
            Ok(html)
        })
        .await;

        let result: Result<String, anyhow::Error> = match raw {
            Ok(inner) => inner,
            Err(join_err) => Err(anyhow::anyhow!("Chrome task panicked: {join_err}")),
        };

        if let Err(ref e) = result {
            info!("fetch_html error for {url}, clearing browser: {e:#}");
            *self.browser.lock().await = None;
        }

        result
    }

    pub async fn fetch_image_urls(&self, url: &str) -> Result<Vec<String>, anyhow::Error> {
        let html = self.fetch_html(url).await?;
        Ok(scrape_attrs(&html, "img[src]", "src"))
    }
}

#[cfg(feature = "chromium")]
impl Default for BrowserManager {
    fn default() -> Self {
        Self::new(30)
    }
}

#[cfg(feature = "chromium")]
impl Clone for BrowserManager {
    fn clone(&self) -> Self {
        Self {
            browser: StdArc::clone(&self.browser),
            fetch_sem: StdArc::clone(&self.fetch_sem),
            wait_time: self.wait_time,
        }
    }
}

#[cfg(not(feature = "chromium"))]
use tracing::warn;

#[cfg(not(feature = "chromium"))]
#[derive(Default, Clone)]
pub struct BrowserManager;

#[cfg(not(feature = "chromium"))]
impl BrowserManager {
    pub fn new(_wait_time_secs: u64) -> Self {
        Self {}
    }

    pub async fn fetch_html(&self, _url: &str) -> Result<String, anyhow::Error> {
        warn!("Chromium feature not enabled");
        Err(anyhow::anyhow!("chromium feature not enabled"))
    }

    pub async fn fetch_image_urls(&self, _url: &str) -> Result<Vec<String>, anyhow::Error> {
        Err(anyhow::anyhow!("chromium feature not enabled"))
    }
}

// ── Utility ──

#[cfg(feature = "chromium")]
fn scrape_attrs(html: &str, selector: &str, attr: &str) -> Vec<String> {
    let doc = scraper::Html::parse_document(html);
    let sel = match scraper::Selector::parse(selector) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    doc.select(&sel)
        .filter_map(|el| el.value().attr(attr).map(|v| v.to_string()))
        .collect()
}
