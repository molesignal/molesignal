// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent Skill 的两个后台任务。
//!
//! - 实例目录扫描：每个节点各读各的磁盘，启动时扫一次，之后每分钟按文件的修改时间判断要不要
//!   重扫。
//! - 仓库自动同步：开了自动同步的仓库每天同步一次，由承担告警角色的节点去做。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::app::skills::SkillService;

const SCAN_EVERY: Duration = Duration::from_secs(60);
const SYNC_CHECK_EVERY: Duration = Duration::from_secs(600);

pub struct SkillPathScanner {
    skills: Arc<SkillService>,
}

impl SkillPathScanner {
    pub fn new(skills: Arc<SkillService>) -> Self {
        Self { skills }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut first = true;

            loop {
                self.scan(first).await;
                first = false;
                tokio::time::sleep(SCAN_EVERY).await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.agent_skill_paths",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "agent_skill_paths")
    )]
    async fn scan(&self, force: bool) {
        let instance = self.skills.instance().clone();

        match tokio::task::spawn_blocking(move || instance.rescan(force)).await {
            Ok(true) => tracing::info!(
                problems = self.skills.instance().problems().len(),
                "instance skill folders scanned"
            ),
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, "instance skill folders could not be scanned"),
        }
    }
}

pub struct SkillRepositorySync {
    skills: Arc<SkillService>,
}

impl SkillRepositorySync {
    pub fn new(skills: Arc<SkillService>) -> Self {
        Self { skills }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(SYNC_CHECK_EVERY).await;
                self.run().await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.agent_skill_repositories",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "agent_skill_repositories")
    )]
    async fn run(&self) {
        let synced = self.skills.sync_due().await;

        if synced > 0 {
            tracing::info!(synced, "skill repositories synced");
        }
    }
}
