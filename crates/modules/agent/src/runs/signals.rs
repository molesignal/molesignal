// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Run notifications within a node.
//!
//! When there is a new event, a stop request, or an answer, the writer sends a notification:
//! directly on the same node, or through PostgreSQL NOTIFY from other nodes. Notifications only
//! identify which Run changes; the content is read from the database. Lost notifications are
//! harmless: the waiting side periodically reads the database itself.

use std::time::Duration;

use kernel::ids::Id;
use tokio::sync::broadcast::{self, error::RecvError};

/// Buffer at most this many unread notifications; beyond this, treat every Run as changed.
const CAPACITY: usize = 1024;

#[derive(Debug, Clone)]
pub struct RunSignalHub {
    tx: broadcast::Sender<String>,
}

impl Default for RunSignalHub {
    fn default() -> Self {
        Self {
            tx: broadcast::channel(CAPACITY).0,
        }
    }
}

impl RunSignalHub {
    pub fn notify(&self, run_id: &str) {
        let _ = self.tx.send(run_id.to_string());
    }

    /// Subscribe before reading the database so notifications arriving after the read are not
    /// missed.
    pub fn subscribe(&self) -> RunSignals {
        RunSignals {
            rx: self.tx.subscribe(),
        }
    }
}

pub struct RunSignals {
    rx: broadcast::Receiver<String>,
}

impl RunSignals {
    /// Wait for a notification for this Run for at most `timeout`; return false on timeout.
    pub async fn changed(&mut self, run_id: &Id, timeout: Duration) -> bool {
        let wait = async {
            loop {
                match self.rx.recv().await {
                    Ok(id) if id == run_id.0 => return true,
                    Ok(_) => {}
                    Err(RecvError::Lagged(_)) => return true,
                    Err(RecvError::Closed) => return std::future::pending().await,
                }
            }
        };

        tokio::time::timeout(timeout, wait).await.unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_waiter_hears_only_about_its_own_run() {
        let hub = RunSignalHub::default();
        let mut signals = hub.subscribe();
        let run = Id::from_string("run-1");

        hub.notify("run-2");
        assert!(!signals.changed(&run, Duration::from_millis(20)).await);

        hub.notify("run-1");
        assert!(signals.changed(&run, Duration::from_millis(20)).await);
    }
}
