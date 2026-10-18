// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 外部 MCP 服务器的定期核对：同步过的服务器每小时连一次，已导入的工具 schema 变了、消失了
//! 或者被标得更危险的标为不可用，prompt 照新的列表更新。由承担告警角色的节点去做。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::app::mcp::McpService;

const CHECK_EVERY: Duration = Duration::from_secs(15 * 60);

pub struct McpResync {
    mcp: Arc<McpService>,
}

impl McpResync {
    pub fn new(mcp: Arc<McpService>) -> Self {
        Self { mcp }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(CHECK_EVERY).await;
                self.run().await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.agent_mcp_resync",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "agent_mcp_resync")
    )]
    async fn run(&self) {
        let checked = self.mcp.resync_due().await;

        if checked > 0 {
            tracing::info!(checked, "MCP servers checked");
        }
    }
}
