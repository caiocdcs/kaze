#[cfg(feature = "chromium")]
use std::sync::Arc as StdArc;
#[cfg(feature = "chromium")]
use std::time::Duration;

#[cfg(feature = "chromium")]
use headless_chrome::{Browser, LaunchOptions};
#[cfg(feature = "chromium")]
use tracing::{debug, info};

#[cfg(feature = "chromium")]
pub struct BrowserManager {
    browser: StdArc<tokio::sync::Mutex<Option<Browser>>>,
    wait_time: Duration,
}

#[cfg(feature = "chromium")]
impl BrowserManager {
    pub fn new(wait_time_secs: u64) -> Self {
        Self {
            browser: StdArc::new(tokio::sync::Mutex::new(None)),
            wait_time: Duration::from_secs(wait_time_secs),
        }
    }

    async fn get_or_create(&self) -> Result<Browser, anyhow::Error> {
        let mut guard = self.browser.lock().await;
        if guard.is_none() {
            info!("Launching headless Chromium (first use)");
            let browser = tokio::task::spawn_blocking(|| {
                Browser::new(LaunchOptions {
                    headless: true,
                    sandbox: true,
                    window_size: Some((1920, 1080)),
                    ..Default::default()
                })
            })
            .await??;
            *guard = Some(browser);
        }
        Ok(guard.as_ref().unwrap().clone())
    }

    pub async fn fetch_html(&self, url: &str) -> Result<String, anyhow::Error> {
        debug!("Fetching HTML via headless Chromium: {url}");
        let browser = self.get_or_create().await?;
        let wait = self.wait_time;
        let url = url.to_string();

        tokio::task::spawn_blocking(move || -> Result<String, anyhow::Error> {
            let tab = browser.new_tab()?;
            tab.navigate_to(&url)?;
            tab.wait_until_navigated()?;
            std::thread::sleep(wait);
            let html = tab.get_content()?;
            tab.close(true)?;
            debug!("Got HTML from Chromium: {} bytes", html.len());
            Ok(html)
        })
        .await?
    }

    pub async fn fetch_image_urls(&self, url: &str) -> Result<Vec<String>, anyhow::Error> {
        let html = self.fetch_html(url).await?;
        Ok(crate::connectors::scrape_attrs(&html, "img[src]", "src"))
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
        warn!(
            "Chromium feature not enabled; cannot render JS pages. Build with --features chromium."
        );
        Err(anyhow::anyhow!("chromium feature not enabled"))
    }

    pub async fn fetch_image_urls(&self, _url: &str) -> Result<Vec<String>, anyhow::Error> {
        Err(anyhow::anyhow!("chromium feature not enabled"))
    }
}
