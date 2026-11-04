// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Sessions, built from views.
//!
//! The intake of `rum_sessions` hands the view rows written to it to the [`SessionMerger`], which
//! answers with one session row for each session they belong to. The merger keeps the sessions it
//! has seen in memory. A session it does not know and that began a while ago was handled by
//! another node, or before a restart: the rows already written for it are read back, and the
//! state is rebuilt from what they hold.

mod state;

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use moka::sync::Cache;
use serde_json::{Map, Value};
use tokio::sync::{Mutex, Semaphore};

use self::state::SessionState;
use crate::{
    domain::{intake::RawEvent, rum::RumSessionMerger},
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// A session that began less than this long ago is taken to be new: there is nothing written for
/// it to read back.
const NEW_SESSION_WINDOW_MS: i64 = 30_000;

/// How far before its newest view a session without a known start may have begun. A session ends
/// after four hours.
const SESSION_LOOKBACK_MS: i64 = 5 * 60 * 60 * 1000;

/// Sessions kept in memory, and for how long after their last view.
const CACHED_SESSIONS: u64 = 100_000;
const CACHE_IDLE: Duration = Duration::from_secs(30 * 60);

/// Rows read back at the same time. A node that has just started meets many sessions at once.
const CONCURRENT_LOADS: usize = 8;

/// The rows already written for a session.
#[async_trait]
pub trait SessionHistory: Send + Sync {
    /// The session rows with this id whose session began in `from_ms..=to_ms`, newest first, with
    /// at least `session.revision`, `session.view_state` and the columns of the session. Empty when
    /// there are none.
    async fn rows(
        &self,
        org_id: &Id,
        session_id: &str,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<Map<String, Value>>>;
}

type Cell = Arc<Mutex<Option<SessionState>>>;

pub struct SessionMerger {
    history: Arc<dyn SessionHistory>,
    sessions: Cache<(Id, String), Cell>,
    loads: Semaphore,
}

impl SessionMerger {
    pub fn new(history: Arc<dyn SessionHistory>) -> Self {
        Self {
            history,
            sessions: Cache::builder()
                .max_capacity(CACHED_SESSIONS)
                .time_to_idle(CACHE_IDLE)
                .build(),
            loads: Semaphore::new(CONCURRENT_LOADS),
        }
    }

    async fn merge_at(
        &self,
        org_id: &Id,
        patches: Vec<RawEvent>,
        now_ms: i64,
    ) -> Result<Vec<RawEvent>> {
        let mut order = Vec::new();
        let mut by_session: HashMap<String, Vec<RawEvent>> = HashMap::new();

        for patch in patches {
            let Some(id) = patch
                .fields
                .get("session.id")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };

            by_session
                .entry(id.clone())
                .or_insert_with(|| {
                    order.push(id);
                    Vec::new()
                })
                .push(patch);
        }

        let mut rows = Vec::with_capacity(order.len());

        for id in order {
            let patches = by_session.remove(&id).unwrap_or_default();

            rows.push(self.merge_session(org_id, &id, patches, now_ms).await);
        }

        Ok(rows)
    }

    async fn merge_session(
        &self,
        org_id: &Id,
        session_id: &str,
        patches: Vec<RawEvent>,
        now_ms: i64,
    ) -> RawEvent {
        let cell = self
            .sessions
            .get_with((org_id.clone(), session_id.to_string()), || {
                Arc::new(Mutex::new(None))
            });

        let mut held = cell.lock().await;

        if held.is_none() {
            *held = Some(self.rebuild(org_id, session_id, &patches, now_ms).await);
        }

        let state = held.get_or_insert_with(SessionState::new);

        for patch in &patches {
            state.apply(patch);
        }

        state.revision = state.revision.saturating_add(1).max(now_ms);
        state.row(session_id, now_ms)
    }

    /// The state of a session this node has no memory of.
    async fn rebuild(
        &self,
        org_id: &Id,
        session_id: &str,
        patches: &[RawEvent],
        now_ms: i64,
    ) -> SessionState {
        let Some((from_ms, to_ms)) = lookup_window(patches, now_ms) else {
            return SessionState::new();
        };

        let Ok(_permit) = self.loads.acquire().await else {
            return SessionState::new();
        };

        match self.history.rows(org_id, session_id, from_ms, to_ms).await {
            Ok(rows) => SessionState::from_rows(&rows),
            Err(error) => {
                // The sessions are derived data; losing the past of one is better than refusing
                // the views it is made of.
                tracing::warn!(%error, session_id, "RUM session history unreadable; starting the session afresh");
                SessionState::new()
            }
        }
    }
}

/// Where to look for the rows of a session, or `None` when it is new.
fn lookup_window(patches: &[RawEvent], now_ms: i64) -> Option<(i64, i64)> {
    let started = patches
        .iter()
        .map(|patch| {
            patch
                .fields
                .get("session.start")
                .and_then(Value::as_i64)
                .unwrap_or(patch.timestamp.0 / 1000)
        })
        .min()?;

    if now_ms.saturating_sub(started) < NEW_SESSION_WINDOW_MS {
        return None;
    }

    let has_start = patches
        .iter()
        .any(|patch| patch.fields.contains_key("session.start"));

    let newest = patches
        .iter()
        .map(|patch| patch.timestamp.0 / 1000)
        .max()
        .unwrap_or(started);

    // The row of a session is dated at its start. With the start given the window is narrow;
    // without it, a session may have begun at any time in its longest lifetime.
    let from = if has_start {
        started - 60_000
    } else {
        newest - SESSION_LOOKBACK_MS
    };

    Some((from, newest.max(now_ms) + 60_000))
}

#[async_trait]
impl RumSessionMerger for SessionMerger {
    async fn merge(&self, org_id: &Id, patches: Vec<RawEvent>) -> Result<Vec<RawEvent>> {
        self.merge_at(org_id, patches, TimestampMicros::now().0 / 1000)
            .await
    }
}

#[cfg(test)]
mod tests;
