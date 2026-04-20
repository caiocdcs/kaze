use crate::id::{EntityId, WorkerId};
use crate::workers::{WorkerEntry, WorkerPayload, WorkerState};

pub fn check_new_chapters(interval_h: u64) -> WorkerEntry {
    WorkerEntry {
        id: WorkerId::new(),
        worker_type: "CheckForNewChapters".into(),
        state: WorkerState::Created,
        depends_on: vec![],
        is_periodic: true,
        interval_secs: Some(interval_h * 3600),
        last_execution: None,
        payload: Some(WorkerPayload::CheckNewChapters),
    }
}

pub fn start_downloads(interval_s: u64) -> WorkerEntry {
    WorkerEntry {
        id: WorkerId::new(),
        worker_type: "StartNewChapterDownloads".into(),
        state: WorkerState::Created,
        depends_on: vec![],
        is_periodic: true,
        interval_secs: Some(interval_s),
        last_execution: None,
        payload: Some(WorkerPayload::StartDownloads),
    }
}

pub fn update_metadata(interval_h: u64) -> WorkerEntry {
    WorkerEntry {
        id: WorkerId::new(),
        worker_type: "UpdateMetadata".into(),
        state: WorkerState::Created,
        depends_on: vec![],
        is_periodic: true,
        interval_secs: Some(interval_h * 3600),
        last_execution: None,
        payload: Some(WorkerPayload::UpdateMetadata),
    }
}

pub fn update_covers(interval_h: u64) -> WorkerEntry {
    WorkerEntry {
        id: WorkerId::new(),
        worker_type: "UpdateCovers".into(),
        state: WorkerState::Created,
        depends_on: vec![],
        is_periodic: true,
        interval_secs: Some(interval_h * 3600),
        last_execution: None,
        payload: Some(WorkerPayload::UpdateCovers),
    }
}

pub fn send_notifications(interval_s: u64) -> WorkerEntry {
    WorkerEntry {
        id: WorkerId::new(),
        worker_type: "SendNotifications".into(),
        state: WorkerState::Created,
        depends_on: vec![],
        is_periodic: true,
        interval_secs: Some(interval_s),
        last_execution: None,
        payload: Some(WorkerPayload::SendNotifications),
    }
}

pub fn update_downloaded(interval_h: u64) -> WorkerEntry {
    WorkerEntry {
        id: WorkerId::new(),
        worker_type: "UpdateChaptersDownloaded".into(),
        state: WorkerState::Created,
        depends_on: vec![],
        is_periodic: true,
        interval_secs: Some(interval_h * 3600),
        last_execution: None,
        payload: Some(WorkerPayload::UpdateDownloaded),
    }
}

pub fn cleanup_covers() -> WorkerEntry {
    WorkerEntry {
        id: WorkerId::new(),
        worker_type: "CleanupMangaCovers".into(),
        state: WorkerState::Created,
        depends_on: vec![],
        is_periodic: true,
        interval_secs: Some(24 * 3600),
        last_execution: None,
        payload: Some(WorkerPayload::CleanupCovers),
    }
}

pub fn cleanup_connector_ids() -> WorkerEntry {
    WorkerEntry {
        id: WorkerId::new(),
        worker_type: "CleanupConnectorIds".into(),
        state: WorkerState::Created,
        depends_on: vec![],
        is_periodic: false,
        interval_secs: None,
        last_execution: None,
        payload: Some(WorkerPayload::CleanupConnectorIds),
    }
}

pub fn remove_old_notifications() -> WorkerEntry {
    WorkerEntry {
        id: WorkerId::new(),
        worker_type: "RemoveOldNotifications".into(),
        state: WorkerState::Created,
        depends_on: vec![],
        is_periodic: true,
        interval_secs: Some(3600),
        last_execution: None,
        payload: Some(WorkerPayload::RemoveOldNotifications),
    }
}
