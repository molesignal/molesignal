// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 节点里关于 Run 的提醒。
//!
//! 有新事件、要求停止、有了回答时，写的那一方发一个提醒：同一节点直接发，其他节点经
//! PostgreSQL NOTIFY 转来。提醒只说哪个 Run 变了，内容去库里读；丢了也不要紧，等的一方隔一会儿
//! 自己去读一次。

use std::time::Duration;

use tokio::sync::broadcast::{self, error::RecvError};

use crate::shared::ids::Id;

/// 来不及看的提醒最多攒这么多；再多时当作每个 Run 都变了。
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

    /// 先订阅、再读库：读完之后到的提醒不会漏掉。
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
    /// 等到这个 Run 有提醒，最多等 `timeout`；超时返回 false。
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
