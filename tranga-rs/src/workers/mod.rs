pub mod download_chapter;
pub mod download_cover;
pub mod retrieve_chapters;
pub mod refresh_libraries;
pub mod move_file;
pub mod periodic;

use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;
use tracing::{info, error};

use crate::app_state::AppState;
use crate::id::WorkerId;
use crate::api::dto::WorkerInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkerState {
    Failed,
    Cancelled,
    Created,
    Waiting,
    Running,
    Completed,
}

pub enum WorkerCommand {
    Submit(WorkerEntry),
    Cancel(WorkerId),
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct WorkerEntry {
    pub id: WorkerId,
    pub worker_type: String,
    pub state: WorkerState,
    pub depends_on: Vec<WorkerId>,
    pub is_periodic: bool,
    pub interval_secs: Option<u64>,
    pub last_execution: Option<String>,
    pub payload: Option<WorkerPayload>,
}

impl WorkerEntry {
    pub fn to_info(&self) -> WorkerInfo {
        WorkerInfo {
            id: self.id.to_string(),
            state: self.state,
            worker_type: self.worker_type.clone(),
            depends_on: self.depends_on.iter().map(|d| d.to_string()).collect(),
            is_periodic: if self.is_periodic { Some(true) } else { None },
            interval_secs: self.interval_secs,
            last_execution: self.last_execution.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum WorkerPayload {
    DownloadChapter { connector_mapping_key: String },
    DownloadCover { connector_mapping_key: String },
    RetrieveChapters { connector_mapping_key: String, language: String },
    RefreshLibraries,
    MoveFile { from: String, to: String },
    CheckNewChapters,
    StartDownloads,
    UpdateMetadata,
    UpdateCovers,
    SendNotifications,
    UpdateDownloaded,
    CleanupCovers,
    CleanupConnectorIds,
    RemoveOldNotifications,
}

pub struct WorkerScheduler {
    workers: HashMap<String, WorkerEntry>,
    running: HashSet<String>,
    downloading_mappings: HashSet<String>,
    cmd_tx: Option<mpsc::UnboundedSender<WorkerCommand>>,
}

impl Default for WorkerScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerScheduler {
    pub fn new() -> Self {
        Self {
            workers: HashMap::new(),
            running: HashSet::new(),
            downloading_mappings: HashSet::new(),
            cmd_tx: None,
        }
    }

    pub fn set_channel(&mut self, tx: mpsc::UnboundedSender<WorkerCommand>) {
        self.cmd_tx = Some(tx);
    }

    pub fn submit(&mut self, entry: WorkerEntry) {
        info!("Submitting worker: {} ({})", entry.id, entry.worker_type);
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(WorkerCommand::Submit(entry));
        } else {
            self.workers.insert(entry.id.to_string(), entry);
        }
    }

    pub fn cancel(&mut self, id: &WorkerId) -> bool {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(WorkerCommand::Cancel(id.clone()));
        }
        if let Some(entry) = self.workers.get_mut(&id.to_string()) {
            entry.state = WorkerState::Cancelled;
            true
        } else {
            false
        }
    }

    pub fn list(&self) -> Vec<WorkerEntry> {
        self.workers.values().cloned().collect()
    }

    pub fn list_by_state(&self, state: WorkerState) -> Vec<WorkerEntry> {
        self.workers.values()
            .filter(|w| w.state == state)
            .cloned()
            .collect()
    }

    pub fn get(&self, id: &WorkerId) -> Option<WorkerEntry> {
        self.workers.get(&id.to_string()).cloned()
    }

    pub fn update_state(&mut self, id: &WorkerId, state: WorkerState) {
        if let Some(entry) = self.workers.get_mut(&id.to_string()) {
            entry.state = state;
        }
    }

    fn mark_running(&mut self, id: &str) {
        self.running.insert(id.to_string());
    }

    fn mark_downloading(&mut self, mapping_key: &str) {
        self.downloading_mappings.insert(mapping_key.to_string());
    }

    fn mark_done(&mut self, id: &str) {
        self.running.remove(id);
    }

    fn mark_download_done(&mut self, mapping_key: &str) {
        self.downloading_mappings.remove(mapping_key);
    }

    pub fn is_downloading(&self, mapping_key: &str) -> bool {
        self.downloading_mappings.contains(mapping_key)
    }
}

pub async fn start_scheduler_loop(state: AppState) -> mpsc::UnboundedSender<WorkerCommand> {
    let (tx, rx) = mpsc::unbounded_channel();

    {
        let mut scheduler = state.scheduler.lock().await;
        scheduler.set_channel(tx.clone());
        for (_, entry) in scheduler.workers.drain() {
            let _ = tx.send(WorkerCommand::Submit(entry));
        }
    }

    let state_clone = state.clone();
    let tx_clone = tx.clone();
    tokio::spawn(scheduler_loop(rx, state_clone, tx_clone));

    tx
}

async fn scheduler_loop(
    mut rx: mpsc::UnboundedReceiver<WorkerCommand>,
    state: AppState,
    tx: mpsc::UnboundedSender<WorkerCommand>,
) {
    let mut pending: Vec<WorkerEntry> = Vec::new();
    let mut periodic_timers: HashMap<String, tokio::time::Interval> = HashMap::new();

    loop {
        tokio::select! {
            Some(cmd) = rx.recv() => {
                match cmd {
                    WorkerCommand::Submit(entry) => {
                        if entry.is_periodic {
                            if let Some(interval_secs) = entry.interval_secs {
                                if interval_secs == 0 { continue; }
                                let id = entry.id.to_string();
                                let interval = tokio::time::interval(
                                    std::time::Duration::from_secs(interval_secs)
                                );
                                periodic_timers.insert(id.clone(), interval);
                                {
                                    let mut scheduler = state.scheduler.lock().await;
                                    scheduler.submit_direct(entry);
                                }
                            }
                        } else {
                            pending.push(entry);
                        }
                    }
                    WorkerCommand::Cancel(id) => {
                        pending.retain(|e| e.id != id);
                        let mut scheduler = state.scheduler.lock().await;
                        scheduler.cancel(&id);
                    }
                    WorkerCommand::Shutdown => {
                        info!("Scheduler shutting down");
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
        }

        while let Some(entry) = pending.pop() {
            let id_str = entry.id.to_string();
            let worker_type = entry.worker_type.clone();

            let download_permit = match &entry.payload {
                Some(WorkerPayload::DownloadChapter { .. }) => {
                    state.download_semaphore.clone()
                }
                _ => state.worker_semaphore.clone(),
            };

            let state_clone = state.clone();
            let entry_clone = entry.clone();
            let tx_for_child = tx.clone();

            let mapping_key = match &entry.payload {
                Some(WorkerPayload::DownloadChapter { connector_mapping_key }) => {
                    Some(connector_mapping_key.clone())
                }
                _ => None,
            };

            tokio::spawn(async move {
                let _permit = download_permit.acquire().await;
                {
                    let mut scheduler = state_clone.scheduler.lock().await;
                    scheduler.update_state(&entry_clone.id, WorkerState::Running);
                    scheduler.mark_running(&id_str);
                    if let Some(ref mk) = mapping_key {
                        scheduler.mark_downloading(mk);
                    }
                }

                info!("Worker {id_str} ({worker_type}) starting");

                let result = execute_worker(&entry_clone, &state_clone).await;

                {
                    let mut scheduler = state_clone.scheduler.lock().await;
                    scheduler.mark_done(&id_str);
                    if let Some(ref mk) = mapping_key {
                        scheduler.mark_download_done(mk);
                    }
                    match result {
                        Ok(child_commands) => {
                            scheduler.update_state(&entry_clone.id, WorkerState::Completed);
                            scheduler.workers.entry(id_str.clone())
                                .and_modify(|e| {
                                    e.last_execution = Some(chrono::Utc::now().to_rfc3339());
                                });
                            for cmd in child_commands {
                                let _ = tx_for_child.send(WorkerCommand::Submit(cmd));
                            }
                        }
                        Err(e) => {
                            error!("Worker {id_str} ({worker_type}) failed: {e}");
                            scheduler.update_state(&entry_clone.id, WorkerState::Failed);
                        }
                    }
                }
            });
        }

        let mut to_run_periodic = Vec::new();
        for (id, interval) in &mut periodic_timers {
            if tokio::time::timeout(std::time::Duration::from_millis(1), interval.tick()).await.is_ok() {
                to_run_periodic.push(id.clone());
            }
        }

        for id in to_run_periodic {
            let scheduler = state.scheduler.lock().await;
            if let Some(entry) = scheduler.workers.get(&id).cloned() {
                let periodic_entry = WorkerEntry {
                    state: WorkerState::Created,
                    ..entry
                };
                pending.push(periodic_entry);
            }
        }
    }
}

impl WorkerScheduler {
    fn submit_direct(&mut self, entry: WorkerEntry) {
        self.workers.insert(entry.id.to_string(), entry);
    }
}

async fn execute_worker(entry: &WorkerEntry, state: &AppState) -> Result<Vec<WorkerEntry>, anyhow::Error> {
    let Some(payload) = &entry.payload else {
        return Ok(vec![]);
    };

    match payload {
        WorkerPayload::DownloadChapter { connector_mapping_key } => {
            download_chapter::execute_download_chapter(state, connector_mapping_key).await
        }
        WorkerPayload::DownloadCover { connector_mapping_key } => {
            download_cover::execute_download_cover(state, connector_mapping_key).await
        }
        WorkerPayload::RetrieveChapters { connector_mapping_key, language } => {
            retrieve_chapters::execute_retrieve_chapters(state, connector_mapping_key, language).await
        }
        WorkerPayload::RefreshLibraries => {
            refresh_libraries::execute_refresh_libraries(state).await
        }
        WorkerPayload::MoveFile { from, to } => {
            move_file::execute_move_file(state, from, to).await
        }
        WorkerPayload::CheckNewChapters => {
            periodic_exec::check_new_chapters(state).await
        }
        WorkerPayload::StartDownloads => {
            periodic_exec::start_downloads(state).await
        }
        WorkerPayload::UpdateMetadata => {
            periodic_exec::update_metadata(state).await
        }
        WorkerPayload::UpdateCovers => {
            periodic_exec::update_covers(state).await
        }
        WorkerPayload::SendNotifications => {
            periodic_exec::send_notifications(state).await
        }
        WorkerPayload::UpdateDownloaded => {
            periodic_exec::update_downloaded(state).await
        }
        WorkerPayload::CleanupCovers => {
            periodic_exec::cleanup_covers(state).await
        }
        WorkerPayload::CleanupConnectorIds => {
            periodic_exec::cleanup_connector_ids(state).await
        }
        WorkerPayload::RemoveOldNotifications => {
            periodic_exec::remove_old_notifications(state).await
        }
    }
}

mod periodic_exec {
    use crate::app_state::AppState;
    use crate::db::chapter_repo::ChapterRepo;
    use crate::db::manga_repo::MangaRepo;
    use crate::db::notification_repo::NotificationRepo;
    use crate::db::metadata_repo::MetadataRepo;
    use crate::db::connector_repo::ConnectorRepo;
    use crate::image::cover_cache::CoverCache;
    use crate::id::{EntityId, WorkerId};
    use crate::workers::{WorkerEntry, WorkerPayload, WorkerState};
    use crate::notification_connectors::NotificationConnector;
    use crate::metadata::MetadataFetcher;
    use tracing::{info, warn};

    pub async fn check_new_chapters(state: &AppState) -> Result<Vec<WorkerEntry>, anyhow::Error> {
        let connectors = MangaRepo::get_connectors_for_download(&state.db).await?;
        let language = state.config.settings.read().await.download_language.clone();
        let mut children = Vec::new();
        for mapping in connectors {
            let id = WorkerId::new();
            children.push(WorkerEntry {
                id,
                worker_type: "RetrieveChapters".into(),
                state: WorkerState::Created,
                depends_on: vec![],
                is_periodic: false,
                interval_secs: None,
                last_execution: None,
                payload: Some(WorkerPayload::RetrieveChapters {
                    connector_mapping_key: mapping.id.clone(),
                    language: language.clone(),
                }),
            });
        }
        Ok(children)
    }

    pub async fn start_downloads(state: &AppState) -> Result<Vec<WorkerEntry>, anyhow::Error> {
        let missing = ChapterRepo::get_missing_downloads(&state.db).await?;
        let scheduler = state.scheduler.lock().await;

        let mut children = Vec::new();
        for mapping in missing {
            if scheduler.is_downloading(&mapping.id) {
                continue;
            }
            let id = WorkerId::new();
            children.push(WorkerEntry {
                id,
                worker_type: "DownloadChapter".into(),
                state: WorkerState::Created,
                depends_on: vec![],
                is_periodic: false,
                interval_secs: None,
                last_execution: None,
                payload: Some(WorkerPayload::DownloadChapter {
                    connector_mapping_key: mapping.id.clone(),
                }),
            });
        }
        Ok(children)
    }

    pub async fn update_metadata(state: &AppState) -> Result<Vec<WorkerEntry>, anyhow::Error> {
        let entries = MetadataRepo::get_entries_for_downloading_manga(&state.db).await?;
        for entry in &entries {
            let fetcher = crate::metadata::MyAnimeListFetcher::new();
            if let Ok(Some(update)) = fetcher.update_metadata(&entry.identifier, &*state.http_client).await {
                if !update.name.is_empty() {
                    let _ = MangaRepo::update_manga_field(&state.db, &entry.manga_id, "name", &update.name).await;
                }
                if !update.description.is_empty() {
                    let _ = MangaRepo::update_manga_field(&state.db, &entry.manga_id, "description", &update.description).await;
                }
                info!("Updated metadata for manga {}", entry.manga_id);
            }
        }
        Ok(vec![])
    }

    pub async fn update_covers(state: &AppState) -> Result<Vec<WorkerEntry>, anyhow::Error> {
        let connectors = MangaRepo::get_connectors_for_download(&state.db).await?;
        let mut children = Vec::new();
        for mapping in connectors {
            let id = WorkerId::new();
            children.push(WorkerEntry {
                id,
                worker_type: "DownloadCover".into(),
                state: WorkerState::Created,
                depends_on: vec![],
                is_periodic: false,
                interval_secs: None,
                last_execution: None,
                payload: Some(WorkerPayload::DownloadCover {
                    connector_mapping_key: mapping.id.clone(),
                }),
            });
        }
        Ok(children)
    }

    pub async fn send_notifications(state: &AppState) -> Result<Vec<WorkerEntry>, anyhow::Error> {
        let unsent = NotificationRepo::list_unsent(&state.db).await?;
        if unsent.is_empty() {
            return Ok(vec![]);
        }

        let connectors = ConnectorRepo::list_notification_connectors(&state.db).await?;
        let mut titles_sent: std::collections::HashSet<String> = std::collections::HashSet::new();

        for notif in &unsent {
            if titles_sent.contains(&notif.title) {
                continue;
            }
            for conn in &connectors {
                let result = crate::notification_connectors::WebhookConnector {
                    name: conn.name.clone(),
                    url: conn.url.clone(),
                    headers: conn.headers.clone(),
                    http_method: conn.http_method.clone(),
                    body: conn.body.clone(),
                }.send_notification(&notif.title, &notif.message).await;

                if let Err(e) = result {
                    warn!("Failed to send notification via {}: {e}", conn.name);
                }
            }
            titles_sent.insert(notif.title.clone());
        }

        NotificationRepo::mark_sent(&state.db).await?;
        Ok(vec![])
    }

    pub async fn update_downloaded(state: &AppState) -> Result<Vec<WorkerEntry>, anyhow::Error> {
        let all_manga = MangaRepo::list(&state.db).await?;
        for manga in &all_manga {
            let chapters = ChapterRepo::get_all_for_manga(&state.db, &manga.id).await?;
            for ch in chapters {
                if let Some(ref file_name) = ch.file_name {
                    let manga_detail = MangaRepo::get_by_id(&state.db, &ch.parent_manga_id).await.ok().flatten();
                    if let Some(detail) = manga_detail
                        && let Some(ref lib_id) = detail.file_library_id {
                            let lib = crate::db::library_repo::LibraryRepo::get_by_id(&state.db, lib_id).await.ok().flatten();
                            if let Some(lib) = lib {
                                let path = std::path::Path::new(&lib.base_path)
                                    .join(crate::utils::clean_name_for_windows(&detail.name))
                                    .join(file_name);
                                let exists = tokio::fs::metadata(&path).await.is_ok();
                                if (ch.downloaded != 0) != exists {
                                    if exists {
                                        let _ = ChapterRepo::mark_downloaded(&state.db, &ch.key, file_name).await;
                                    } else {
                                        let _ = ChapterRepo::mark_not_downloaded(&state.db, &ch.key).await;
                                    }
                                }
                            }
                        }
                }
            }
        }
        Ok(vec![])
    }

    pub async fn cleanup_covers(state: &AppState) -> Result<Vec<WorkerEntry>, anyhow::Error> {
        let all_manga = MangaRepo::list(&state.db).await?;
        let retain: Vec<String> = all_manga.iter()
            .filter_map(|m| m.connectors.iter().find_map(|c| {
                if c.use_for_download { None } else { Some(String::new()) }
            }))
            .collect();

        let cache = CoverCache::new(state.config.working_dir.join("imageCache"));
        cache.cleanup_unused(&retain).await?;
        Ok(vec![])
    }

    pub async fn cleanup_connector_ids(state: &AppState) -> Result<Vec<WorkerEntry>, anyhow::Error> {
        let connectors = state.connectors.read().await;
        let valid_names: std::collections::HashSet<String> = connectors.all()
            .iter().map(|c| c.name().to_lowercase()).collect();

        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT key, manga_connector_name, obj_id FROM manga_connector_to_chapter"
        )
        .fetch_all(&state.db)
        .await?;

        for (key, name, _obj_id) in rows {
            if !valid_names.contains(&name.to_lowercase()) {
                sqlx::query("DELETE FROM manga_connector_to_chapter WHERE key = $1")
                    .bind(&key)
                    .execute(&state.db)
                    .await?;
            }
        }
        Ok(vec![])
    }

    pub async fn remove_old_notifications(state: &AppState) -> Result<Vec<WorkerEntry>, anyhow::Error> {
        NotificationRepo::delete_sent(&state.db).await?;
        Ok(vec![])
    }
}
