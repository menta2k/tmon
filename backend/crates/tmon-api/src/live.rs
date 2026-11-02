//! `GET /live` — SSE stream of 5-second ticks and warnings (FR-016, FR-019).

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{self, Stream};
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

use crate::state::AppState;
use crate::warnings::{Warning, current};

pub const KEEP_ALIVE: Duration = Duration::from_secs(15);

struct Cursor {
    state: AppState,
    rx: tokio::sync::broadcast::Receiver<tmon_enrich::Tick>,
    last_warnings: Option<Vec<Warning>>,
    pending: Vec<Event>,
}

fn json_event(name: &str, data: &impl serde::Serialize) -> Option<Event> {
    match Event::default().event(name).json_data(data) {
        Ok(e) => Some(e),
        Err(e) => {
            tracing::error!(error = %e, "cannot encode SSE event");
            None
        }
    }
}

impl Cursor {
    /// Queue a `warning` event when the warning set changed.
    fn check_warnings(&mut self) {
        let now = current(&self.state);
        if self.last_warnings.as_ref() != Some(&now) {
            self.pending.extend(json_event("warning", &now));
            self.last_warnings = Some(now);
        }
    }

    async fn next(mut self) -> Option<(Event, Self)> {
        loop {
            if let Some(e) = self.pending.pop() {
                return Some((e, self));
            }
            match self.rx.recv().await {
                Ok(tick) => {
                    self.pending.extend(json_event("tick", &tick));
                    self.check_warnings();
                    self.pending.reverse();
                }
                Err(RecvError::Lagged(n)) => tracing::debug!(skipped = n, "live subscriber lagged"),
                Err(RecvError::Closed) => return None,
            }
        }
    }
}

pub async fn stream(
    State(s): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut cursor = Cursor {
        rx: s.live.subscribe(),
        state: s,
        last_warnings: None,
        pending: Vec::new(),
    };
    cursor.check_warnings();
    let events = stream::unfold(cursor, |c| async move {
        c.next().await.map(|(e, c)| (Ok(e), c))
    });
    Sse::new(events).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
}
