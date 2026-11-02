//! Single-writer storage actor: buffers enriched flows and flushes them every
//! `flush_interval` (or when the buffer is large), rotating hours as needed.

use std::sync::Arc;
use std::time::Duration;
use tmon_core::FlowRecord;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::{DuckStore, now_ms};
use crate::{FlowStore, StoreError, StoreResult};

/// Flush early once this many flows are buffered.
pub const MAX_BUFFER: usize = 50_000;
const CHANNEL_CAPACITY: usize = 1024;

enum Msg {
    Batch(Vec<FlowRecord>),
    Flush(oneshot::Sender<StoreResult<()>>),
}

/// Cloneable handle to the writer actor.
#[derive(Clone)]
pub struct WriterHandle {
    tx: mpsc::Sender<Msg>,
}

impl WriterHandle {
    /// Queue a batch (awaits when the actor is saturated).
    pub async fn send(&self, flows: Vec<FlowRecord>) -> StoreResult<()> {
        self.tx.send(Msg::Batch(flows)).await.map_err(|_| closed())
    }

    /// Flush buffered flows now and wait for completion.
    pub async fn flush(&self) -> StoreResult<()> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Msg::Flush(tx)).await.map_err(|_| closed())?;
        rx.await.map_err(|_| closed())?
    }
}

fn closed() -> StoreError {
    StoreError::Task("writer stopped".into())
}

/// Spawn the actor. It stops (after a final flush) when all handles are dropped.
pub fn spawn(store: Arc<DuckStore>, flush_every: Duration) -> (WriterHandle, JoinHandle<()>) {
    let (tx, mut rx) = mpsc::channel::<Msg>(CHANNEL_CAPACITY);
    let handle = tokio::spawn(async move {
        let mut buf: Vec<FlowRecord> = Vec::new();
        let mut tick = tokio::time::interval(flush_every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(Msg::Batch(v)) => {
                        buf.extend(v);
                        if buf.len() >= MAX_BUFFER {
                            log_err(flush(&store, std::mem::take(&mut buf)).await);
                        }
                    }
                    Some(Msg::Flush(reply)) => {
                        let _ = reply.send(flush(&store, std::mem::take(&mut buf)).await);
                    }
                    None => break,
                },
                _ = tick.tick() => log_err(flush(&store, std::mem::take(&mut buf)).await),
            }
        }
        log_err(flush(&store, buf).await);
    });
    (WriterHandle { tx }, handle)
}

fn log_err(r: StoreResult<()>) {
    if let Err(e) = r {
        tracing::error!(error = %e, "storage flush failed");
    }
}

/// Persist a batch and rotate completed hours (blocking work off the runtime).
async fn flush(store: &Arc<DuckStore>, flows: Vec<FlowRecord>) -> StoreResult<()> {
    let store = Arc::clone(store);
    tokio::task::spawn_blocking(move || {
        store.append_batch(&flows)?;
        store.rotate(now_ms()).map(|_| ())
    })
    .await
    .map_err(|e| StoreError::Task(e.to_string()))?
}
