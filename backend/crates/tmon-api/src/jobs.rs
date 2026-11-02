//! In-memory background jobs, executed one at a time (FR-023 re-apply).

use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Finished jobs kept for polling.
const KEEP_FINISHED: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Job {
    pub id: u64,
    pub state: JobState,
    pub progress: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub type Progress<'a> = &'a dyn Fn(f64);
pub type Work = Box<dyn FnOnce(Progress<'_>) -> Result<(), String> + Send>;

pub struct Jobs {
    table: Mutex<BTreeMap<u64, Job>>,
    tx: mpsc::UnboundedSender<(u64, Work)>,
}

impl Jobs {
    /// Create the job table and its single worker task.
    pub fn spawn() -> Arc<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel::<(u64, Work)>();
        let jobs = Arc::new(Self {
            table: Mutex::new(BTreeMap::new()),
            tx,
        });
        let me = Arc::downgrade(&jobs);
        tokio::spawn(async move {
            while let Some((id, work)) = rx.recv().await {
                let Some(j) = me.upgrade() else { break };
                j.set(id, |job| Job {
                    state: JobState::Running,
                    ..job
                });
                let runner = Arc::clone(&j);
                let result = tokio::task::spawn_blocking(move || {
                    let progress = |p: f64| {
                        runner.set(id, |job| Job {
                            progress: p.clamp(0.0, 1.0),
                            ..job
                        })
                    };
                    work(&progress)
                })
                .await
                .unwrap_or_else(|e| Err(format!("job panicked: {e}")));
                j.set(id, |job| match &result {
                    Ok(()) => Job {
                        state: JobState::Done,
                        progress: 1.0,
                        ..job
                    },
                    Err(e) => Job {
                        state: JobState::Failed,
                        error: Some(e.clone()),
                        ..job
                    },
                });
                if let Err(e) = result {
                    tracing::error!(job = id, error = %e, "background job failed");
                }
            }
        });
        jobs
    }

    fn set(&self, id: u64, f: impl FnOnce(Job) -> Job) {
        if let Ok(mut t) = self.table.lock() {
            if let Some(cur) = t.get(&id).cloned() {
                t.insert(id, f(cur));
            }
        }
    }

    pub fn submit(&self, work: Work) -> Result<u64, String> {
        let mut t = self
            .table
            .lock()
            .map_err(|_| "job table poisoned".to_string())?;
        let id = t.keys().next_back().map_or(1, |k| k + 1);
        t.insert(
            id,
            Job {
                id,
                state: JobState::Queued,
                progress: 0.0,
                error: None,
            },
        );
        let finished: Vec<u64> = t
            .values()
            .filter(|j| matches!(j.state, JobState::Done | JobState::Failed))
            .map(|j| j.id)
            .collect();
        for old in finished
            .iter()
            .take(finished.len().saturating_sub(KEEP_FINISHED))
        {
            t.remove(old);
        }
        drop(t);
        self.tx
            .send((id, work))
            .map_err(|_| "job worker stopped".to_string())?;
        Ok(id)
    }

    pub fn get(&self, id: u64) -> Option<Job> {
        self.table.lock().ok()?.get(&id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn wait(jobs: &Jobs, id: u64) -> Job {
        for _ in 0..100 {
            let j = jobs.get(id).unwrap();
            if matches!(j.state, JobState::Done | JobState::Failed) {
                return j;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("job {id} did not finish");
    }

    #[tokio::test]
    async fn runs_jobs_and_reports_progress_and_failure() {
        let jobs = Jobs::spawn();
        let ok = jobs
            .submit(Box::new(|p| {
                p(0.5);
                Ok(())
            }))
            .unwrap();
        let bad = jobs.submit(Box::new(|_| Err("boom".into()))).unwrap();
        assert_eq!(
            wait(&jobs, ok).await,
            Job {
                id: ok,
                state: JobState::Done,
                progress: 1.0,
                error: None
            }
        );
        let b = wait(&jobs, bad).await;
        assert_eq!(b.state, JobState::Failed);
        assert_eq!(b.error.as_deref(), Some("boom"));
        assert!(jobs.get(999).is_none());
    }
}
