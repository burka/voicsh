//! In-memory transcription job coordination for daemon file transcription.

use crate::ipc::protocol::{JobInfo, JobState};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, Semaphore, watch};

const DEFAULT_MAX_ACTIVE_JOBS: usize = 64;
const DEFAULT_MAX_CONCURRENT_JOBS: usize = 1;
const DEFAULT_MAX_RETAINED_TERMINAL_JOBS: usize = 1000;

#[derive(Debug)]
pub struct JobEntry {
    info: Mutex<JobInfo>,
    tx: watch::Sender<JobInfo>,
    cancel_requested: AtomicBool,
    active_slot_released: AtomicBool,
}

impl JobEntry {
    fn new(info: JobInfo) -> Self {
        let (tx, _) = watch::channel(info.clone());
        Self {
            info: Mutex::new(info),
            tx,
            cancel_requested: AtomicBool::new(false),
            active_slot_released: AtomicBool::new(false),
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<JobInfo> {
        self.tx.subscribe()
    }

    pub async fn snapshot(&self) -> JobInfo {
        self.info.lock().await.clone()
    }

    pub async fn set_state(
        &self,
        state: JobState,
        text: Option<String>,
        error: Option<String>,
    ) -> JobInfo {
        let now = now_ms();
        let mut info = self.info.lock().await;
        info.state = state;
        info.updated_at_ms = now;
        match state {
            JobState::Running => {
                info.started_at_ms.get_or_insert(now);
            }
            state if state.is_terminal() => {
                info.finished_at_ms.get_or_insert(now);
            }
            _ => {}
        }
        if let Some(text) = text {
            info.text = Some(text);
        }
        if let Some(error) = error {
            info.error = Some(error);
        }
        let snapshot = info.clone();
        if self.tx.send(snapshot.clone()).is_err() {
            // No subscribers are currently attached; the stored snapshot remains authoritative.
        }
        snapshot
    }

    pub fn request_cancel(&self) {
        self.cancel_requested.store(true, Ordering::Release);
    }

    pub fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub struct JobManager {
    jobs: Mutex<HashMap<String, Arc<JobEntry>>>,
    next_id: AtomicU64,
    active_jobs: AtomicUsize,
    max_active_jobs: usize,
    max_retained_terminal_jobs: usize,
    semaphore: Arc<Semaphore>,
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new(
            DEFAULT_MAX_ACTIVE_JOBS,
            DEFAULT_MAX_CONCURRENT_JOBS,
            DEFAULT_MAX_RETAINED_TERMINAL_JOBS,
        )
    }
}

impl JobManager {
    pub fn new(
        max_active_jobs: usize,
        max_concurrent_jobs: usize,
        max_retained_terminal_jobs: usize,
    ) -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            active_jobs: AtomicUsize::new(0),
            max_active_jobs,
            max_retained_terminal_jobs,
            semaphore: Arc::new(Semaphore::new(max_concurrent_jobs.max(1))),
        }
    }

    pub async fn submit(&self, path: String) -> Result<Arc<JobEntry>, String> {
        if self
            .active_jobs
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < self.max_active_jobs).then_some(count + 1)
            })
            .is_err()
        {
            return Err(format!(
                "Transcription job queue is full (max {} active jobs)",
                self.max_active_jobs
            ));
        }

        let id = self.next_id.fetch_add(1, Ordering::AcqRel);
        let now = now_ms();
        let info = JobInfo {
            job_id: format!("job-{id}"),
            state: JobState::Queued,
            path,
            created_at_ms: now,
            updated_at_ms: now,
            started_at_ms: None,
            finished_at_ms: None,
            text: None,
            error: None,
        };
        let entry = Arc::new(JobEntry::new(info.clone()));
        self.jobs
            .lock()
            .await
            .insert(info.job_id.clone(), Arc::clone(&entry));
        Ok(entry)
    }

    pub fn semaphore(&self) -> Arc<Semaphore> {
        Arc::clone(&self.semaphore)
    }

    pub async fn get(&self, job_id: &str) -> Option<Arc<JobEntry>> {
        self.jobs.lock().await.get(job_id).cloned()
    }

    pub async fn list(&self, state: Option<JobState>) -> Vec<JobInfo> {
        let entries: Vec<Arc<JobEntry>> = self.jobs.lock().await.values().cloned().collect();
        let mut jobs = Vec::with_capacity(entries.len());
        for entry in entries {
            let info = entry.snapshot().await;
            if state.is_none_or(|wanted| info.state == wanted) {
                jobs.push(info);
            }
        }
        jobs.sort_by_key(|job| job.created_at_ms);
        jobs
    }

    pub async fn cancel(&self, job_id: &str) -> Option<JobInfo> {
        let entry = self.get(job_id).await?;
        entry.request_cancel();
        let current = entry.snapshot().await;
        if current.state == JobState::Queued {
            let canceled = entry
                .set_state(JobState::Canceled, None, Some("Job canceled".to_string()))
                .await;
            self.release_active_slot(&entry);
            self.prune_terminal_jobs().await;
            Some(canceled)
        } else {
            Some(current)
        }
    }

    pub fn release_active_slot(&self, entry: &JobEntry) {
        if !entry.active_slot_released.swap(true, Ordering::AcqRel) {
            self.active_jobs.fetch_sub(1, Ordering::AcqRel);
        }
    }

    pub async fn prune_terminal_jobs(&self) {
        let entries: Vec<(String, Arc<JobEntry>)> = self
            .jobs
            .lock()
            .await
            .iter()
            .map(|(id, entry)| (id.clone(), Arc::clone(entry)))
            .collect();

        let mut terminal_jobs = Vec::new();
        for (id, entry) in entries {
            let info = entry.snapshot().await;
            if info.state.is_terminal() {
                terminal_jobs.push((info.finished_at_ms.unwrap_or(info.updated_at_ms), id));
            }
        }

        if terminal_jobs.len() <= self.max_retained_terminal_jobs {
            return;
        }

        terminal_jobs.sort_by_key(|(finished_at_ms, _)| *finished_at_ms);
        let remove_count = terminal_jobs.len() - self.max_retained_terminal_jobs;
        let remove_ids: std::collections::HashSet<String> = terminal_jobs
            .into_iter()
            .take(remove_count)
            .map(|(_, id)| id)
            .collect();

        self.jobs
            .lock()
            .await
            .retain(|id, _entry| !remove_ids.contains(id));
    }
}

fn now_ms() -> u64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn submit_creates_queued_job_and_replayable_subscription() {
        let manager = JobManager::new(4, 1, 1000);
        let entry = manager.submit("/tmp/a.wav".to_string()).await.unwrap();
        let rx = entry.subscribe();

        let snapshot = rx.borrow().clone();
        assert_eq!(snapshot.state, JobState::Queued);
        assert_eq!(snapshot.path, "/tmp/a.wav");
        assert_eq!(snapshot.job_id, "job-1");
    }

    #[tokio::test]
    async fn manager_rejects_when_active_queue_is_full() {
        let manager = JobManager::new(1, 1, 1000);
        manager.submit("/tmp/a.wav".to_string()).await.unwrap();
        let error = manager.submit("/tmp/b.wav".to_string()).await.unwrap_err();
        assert!(error.contains("queue is full"));
    }

    #[tokio::test]
    async fn cancel_queued_job_publishes_terminal_update() {
        let manager = JobManager::new(4, 1, 1000);
        let entry = manager.submit("/tmp/a.wav".to_string()).await.unwrap();
        let mut rx = entry.subscribe();

        let canceled = manager.cancel("job-1").await.unwrap();
        assert_eq!(canceled.state, JobState::Canceled);
        rx.changed().await.unwrap();
        assert_eq!(rx.borrow().state, JobState::Canceled);
        assert!(rx.borrow().state.is_terminal());
    }

    #[tokio::test]
    async fn list_filters_by_state() {
        let manager = JobManager::new(4, 1, 1000);
        let first = manager.submit("/tmp/a.wav".to_string()).await.unwrap();
        let _second = manager.submit("/tmp/b.wav".to_string()).await.unwrap();
        first
            .set_state(JobState::Done, Some("done".to_string()), None)
            .await;

        let done = manager.list(Some(JobState::Done)).await;
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].job_id, "job-1");
    }

    #[tokio::test]
    async fn cancel_queued_job_releases_queue_capacity_immediately() {
        let manager = JobManager::new(1, 1, 1000);
        manager.submit("/tmp/a.wav".to_string()).await.unwrap();
        manager.cancel("job-1").await.unwrap();

        let next = manager.submit("/tmp/b.wav".to_string()).await;
        assert!(
            next.is_ok(),
            "canceling a queued job should free active queue capacity"
        );
    }

    #[tokio::test]
    async fn terminal_job_pruning_keeps_newest_snapshots() {
        let manager = JobManager::new(4, 1, 1);
        let first = manager.submit("/tmp/a.wav".to_string()).await.unwrap();
        first
            .set_state(JobState::Done, Some("first".to_string()), None)
            .await;
        manager.release_active_slot(&first);

        let second = manager.submit("/tmp/b.wav".to_string()).await.unwrap();
        second
            .set_state(JobState::Done, Some("second".to_string()), None)
            .await;
        manager.release_active_slot(&second);
        manager.prune_terminal_jobs().await;

        assert!(manager.get("job-1").await.is_none());
        assert!(manager.get("job-2").await.is_some());
    }
}
