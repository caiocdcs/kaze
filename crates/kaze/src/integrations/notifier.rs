//! Turns the event stream into integration jobs.
//!
//! Events are batched: a backfill of 200 chapters becomes one "Downloaded
//! 200 chapters" message and one library scan, not 200 of each. A batch is
//! sent once things have been quiet for `quiet`, or at the latest `max_wait`
//! after its first event, so a long backfill still reports progress.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::Duration,
};

use sqlx::SqlitePool;
use tokio::{
    sync::broadcast::{Receiver, error::RecvError},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use super::{Message, Trigger};
use crate::{
    db,
    events::Event,
    jobs::{Job, JobQueue},
};

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub quiet: Duration,
    pub max_wait: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            quiet: Duration::from_secs(30),
            max_wait: Duration::from_secs(5 * 60),
        }
    }
}

/// Events collected since the last flush.
#[derive(Debug, Default)]
struct Batch {
    /// series → chapters downloaded
    downloaded: BTreeMap<String, Vec<String>>,
    /// "series chapter: error" lines, for chapters out of retries
    failed: Vec<String>,
    /// Series of the failed chapters
    failed_ids: BTreeSet<i64>,
    /// series → new chapters found
    found: BTreeMap<String, usize>,
    /// "Source: error" lines, for sources that just broke
    broken: Vec<String>,
    /// series → id, for links into the UI
    ids: BTreeMap<String, i64>,
    /// Files were added or removed, so library servers should scan.
    library_changed: bool,
    first_at: Option<Instant>,
    last_at: Option<Instant>,
}

impl Batch {
    fn is_empty(&self) -> bool {
        self.first_at.is_none()
    }

    fn touch(&mut self) {
        let now = Instant::now();
        self.first_at.get_or_insert(now);
        self.last_at = Some(now);
    }

    fn deadline(&self, timing: Timing) -> Option<Instant> {
        Some((self.last_at? + timing.quiet).min(self.first_at? + timing.max_wait))
    }

    fn ids_of<'a>(&self, series: impl Iterator<Item = &'a String>) -> Vec<i64> {
        series.filter_map(|s| self.ids.get(s).copied()).collect()
    }

    fn messages(&self) -> Vec<Message> {
        let mut out = Vec::new();
        let count: usize = self.downloaded.values().map(Vec::len).sum();
        if count > 0 {
            let title = match (count, self.downloaded.iter().next()) {
                (1, Some((series, chapters))) => {
                    format!("{series} chapter {} downloaded", chapters[0])
                }
                _ => format!("Downloaded {count} chapters"),
            };
            let body = self
                .downloaded
                .iter()
                .map(|(series, chapters)| format!("{series}: {}", list(chapters)))
                .collect::<Vec<_>>()
                .join("\n");
            out.push(Message {
                trigger: Trigger::Downloaded,
                title,
                body,
                count,
                series: self.downloaded.keys().cloned().collect(),
                series_ids: self.ids_of(self.downloaded.keys()),
            });
        }
        if !self.failed.is_empty() {
            let count = self.failed.len();
            out.push(Message {
                trigger: Trigger::Failed,
                title: if count == 1 {
                    "A chapter failed to download".into()
                } else {
                    format!("{count} chapters failed to download")
                },
                body: self.failed.join("\n"),
                count,
                series: Vec::new(),
                series_ids: self.failed_ids.iter().copied().collect(),
            });
        }
        if !self.found.is_empty() {
            let count: usize = self.found.values().sum();
            out.push(Message {
                trigger: Trigger::Found,
                title: format!("{count} new chapter(s)"),
                body: self
                    .found
                    .iter()
                    .map(|(series, n)| format!("{series}: {n} new"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                count,
                series: self.found.keys().cloned().collect(),
                series_ids: self.ids_of(self.found.keys()),
            });
        }
        if !self.broken.is_empty() {
            let count = self.broken.len();
            out.push(Message {
                trigger: Trigger::SourceBroken,
                title: if count == 1 {
                    "A source looks broken".into()
                } else {
                    format!("{count} sources look broken")
                },
                body: self.broken.join("\n"),
                count,
                series: Vec::new(),
                series_ids: Vec::new(),
            });
        }
        out
    }
}

/// "1, 2, 3" or "1, 2, … 40 (12 chapters)" for long runs.
fn list(chapters: &[String]) -> String {
    if chapters.len() <= 6 {
        return chapters.join(", ");
    }
    format!(
        "{}, … {} ({} chapters)",
        chapters[..3].join(", "),
        chapters[chapters.len() - 1],
        chapters.len()
    )
}

pub async fn run(
    db: SqlitePool,
    mut rx: Receiver<Event>,
    jobs: JobQueue,
    timing: Timing,
    shutdown: CancellationToken,
) {
    let mut batch = Batch::default();
    // Attempts that failed, kept until we know whether the job retries.
    let mut attempts = Attempts::new();
    loop {
        let deadline = batch.deadline(timing);
        let sleep = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = sleep => {
                flush(&db, &jobs, &std::mem::take(&mut batch)).await;
            }
            event = rx.recv() => match event {
                Ok(event) => collect(&mut batch, &mut attempts, event),
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "notifier fell behind");
                }
                Err(RecvError::Closed) => return,
            },
        }
    }
}

/// job → (series id, "series chapter: error")
type Attempts = HashMap<i64, (i64, String)>;

fn collect(batch: &mut Batch, attempts: &mut Attempts, event: Event) {
    match event {
        Event::ChapterDownloaded {
            series_id,
            series,
            chapter,
            ..
        } => {
            batch.ids.insert(series.clone(), series_id);
            batch.downloaded.entry(series).or_default().push(chapter);
            batch.library_changed = true;
            batch.touch();
        }
        Event::FilesDeleted { .. } => {
            batch.library_changed = true;
            batch.touch();
        }
        Event::ChaptersFound {
            series_id,
            series,
            count,
        } => {
            batch.ids.insert(series.clone(), series_id);
            *batch.found.entry(series).or_default() += count;
            batch.touch();
        }
        Event::ChapterFailed {
            job_id,
            series_id,
            series,
            chapter,
            error,
            ..
        } => {
            let line = format!("{series} chapter {chapter}: {error}");
            attempts.insert(job_id, (series_id, line));
        }
        Event::JobFailed {
            job_id, will_retry, ..
        } => {
            if let Some((series_id, line)) = attempts.remove(&job_id)
                && !will_retry
            {
                batch.failed.push(line);
                batch.failed_ids.insert(series_id);
                batch.touch();
            }
        }
        Event::SourceBroken {
            name,
            failures,
            error,
            ..
        } => {
            batch.broken.push(format!(
                "{name}: {failures} failures in a row, last: {error}"
            ));
            batch.touch();
        }
        Event::JobFinished { job_id } | Event::JobCancelled { job_id } => {
            attempts.remove(&job_id);
        }
        _ => {}
    }
}

async fn flush(db: &SqlitePool, jobs: &JobQueue, batch: &Batch) {
    if batch.is_empty() {
        return;
    }
    let integrations = match db::integrations::list(db).await {
        Ok(list) => list,
        Err(err) => {
            tracing::error!("could not load integrations: {err}");
            return;
        }
    };
    let messages = batch.messages();
    for integration in integrations.into_iter().filter(|i| i.enabled) {
        let integration_id = integration.id;
        let jobs_for: Vec<Job> = if integration.config.is_library() {
            if batch.library_changed {
                vec![Job::ScanLibrary { integration_id }]
            } else {
                Vec::new()
            }
        } else {
            messages
                .iter()
                .filter(|m| integration.events.contains(&m.trigger))
                .map(|m| Job::Notify {
                    integration_id,
                    message: m.clone(),
                })
                .collect()
        };
        for job in jobs_for {
            if let Err(err) = jobs.submit(job).await {
                tracing::error!("could not queue integration job: {err}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn downloaded(series: &str, chapter: &str) -> Event {
        Event::ChapterDownloaded {
            series_id: if series == "Berserk" { 1 } else { 2 },
            chapter_id: 1,
            series: series.into(),
            chapter: chapter.into(),
            path: String::new(),
        }
    }

    #[test]
    fn batches_downloads_into_one_message() {
        let mut batch = Batch::default();
        let mut attempts = HashMap::new();
        for ch in ["1", "2", "3"] {
            collect(&mut batch, &mut attempts, downloaded("Berserk", ch));
        }
        collect(&mut batch, &mut attempts, downloaded("Frieren", "140"));
        let messages = batch.messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].title, "Downloaded 4 chapters");
        assert_eq!(messages[0].body, "Berserk: 1, 2, 3\nFrieren: 140");
        assert_eq!(messages[0].series, ["Berserk", "Frieren"]);
        assert_eq!(messages[0].series_ids, [1, 2]);
    }

    #[test]
    fn deleting_files_scans_without_a_message() {
        let mut batch = Batch::default();
        let deleted = Event::FilesDeleted {
            series_id: None,
            series: "Berserk".into(),
            count: 3,
        };
        collect(&mut batch, &mut HashMap::new(), deleted);
        assert!(batch.library_changed);
        assert_eq!(batch.messages(), []);
    }

    #[test]
    fn one_download_names_the_chapter() {
        let mut batch = Batch::default();
        collect(
            &mut batch,
            &mut HashMap::new(),
            downloaded("Berserk", "374"),
        );
        assert_eq!(batch.messages()[0].title, "Berserk chapter 374 downloaded");
    }

    #[test]
    fn only_final_failures_are_reported() {
        let mut batch = Batch::default();
        let mut attempts = HashMap::new();
        let failed = |job_id| Event::ChapterFailed {
            job_id,
            series_id: 1,
            chapter_id: job_id,
            series: "Berserk".into(),
            chapter: job_id.to_string(),
            error: "HTTP 503".into(),
        };
        let gave_up = |job_id, will_retry| Event::JobFailed {
            job_id,
            error: "HTTP 503".into(),
            will_retry,
        };
        collect(&mut batch, &mut attempts, failed(1));
        collect(&mut batch, &mut attempts, gave_up(1, true));
        assert!(batch.is_empty(), "a retried attempt is not news");
        collect(&mut batch, &mut attempts, failed(2));
        collect(&mut batch, &mut attempts, gave_up(2, false));
        assert_eq!(batch.failed, ["Berserk chapter 2: HTTP 503"]);
        assert_eq!(batch.messages()[0].series_ids, [1]);
        assert!(attempts.is_empty());
    }

    #[test]
    fn broken_sources_become_one_message() {
        let mut batch = Batch::default();
        let broken = |name: &str| Event::SourceBroken {
            source: name.to_lowercase(),
            name: name.into(),
            failures: 3,
            error: "HTTP 503".into(),
        };
        collect(&mut batch, &mut HashMap::new(), broken("WeebCentral"));
        let messages = batch.messages();
        assert_eq!(messages[0].trigger, Trigger::SourceBroken);
        assert_eq!(messages[0].title, "A source looks broken");
        assert_eq!(
            messages[0].body,
            "WeebCentral: 3 failures in a row, last: HTTP 503"
        );
        collect(&mut batch, &mut HashMap::new(), broken("MangaDex"));
        assert_eq!(batch.messages()[0].title, "2 sources look broken");
        assert!(!batch.library_changed);
    }

    #[test]
    fn long_chapter_lists_are_shortened() {
        let chapters: Vec<String> = (1..=40).map(|n| n.to_string()).collect();
        assert_eq!(list(&chapters), "1, 2, 3, … 40 (40 chapters)");
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_waits_for_quiet_but_not_forever() {
        let timing = Timing {
            quiet: Duration::from_secs(30),
            max_wait: Duration::from_secs(300),
        };
        let mut batch = Batch::default();
        assert_eq!(batch.deadline(timing), None);
        let start = Instant::now();
        batch.touch();
        assert_eq!(batch.deadline(timing), Some(start + timing.quiet));
        // Keep events coming every 20 s: the quiet window keeps moving...
        for _ in 0..20 {
            tokio::time::advance(Duration::from_secs(20)).await;
            batch.touch();
        }
        // ...but the batch still goes out max_wait after its first event.
        assert_eq!(batch.deadline(timing), Some(start + timing.max_wait));
    }
}
