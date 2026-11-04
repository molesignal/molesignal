// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashSet, sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    app::{
        cluster::{PeerRole, WorkOwnership},
        status_page::{DependencySignalQueue, StatusPageService},
    },
    shared::ids::Id,
};

/// 收到变化后最多等这么久再评估，把同一阵变化合并成一次。
const COALESCE: Duration = Duration::from_secs(2);

/// alert 主节点多久把全部组织评估一遍，补上本节点没收到的变化。
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// 组件依赖评估：告警事件或拨测状态变化后，评估所在组织有依赖的状态页组件，把变化交给跟随它的
/// 组件和页面的自动化规则。每个节点评估自己收到的变化；alert 主节点（见
/// [`WorkOwnership::is_primary`]）另外定期全量评估。健康度以比较后写入，多个节点同时评估也只记
/// 一次变化。
pub struct StatusPageDependencyWorker {
    status_pages: Arc<StatusPageService>,
    signals: Arc<DependencySignalQueue>,
    /// 默认不分担（本节点就是主节点）。
    ownership: Arc<WorkOwnership>,
    /// 本节点是否承担 alert 角色、参与全量评估。
    sweeps: bool,
}

impl StatusPageDependencyWorker {
    pub fn new(status_pages: Arc<StatusPageService>, signals: Arc<DependencySignalQueue>) -> Self {
        Self {
            status_pages,
            signals,
            ownership: WorkOwnership::solo(PeerRole::Alert),
            sweeps: true,
        }
    }

    /// 与其余 alert 节点分担全量评估；`sweeps` 为假的节点只评估自己收到的变化。
    pub fn with_ownership(mut self, ownership: Arc<WorkOwnership>, sweeps: bool) -> Self {
        self.ownership = ownership;
        self.sweeps = sweeps;
        self
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut last_sweep: Option<tokio::time::Instant> = None;

            loop {
                self.signals.wait(COALESCE).await;
                tokio::time::sleep(COALESCE).await;
                let mut organizations: HashSet<Id> = self.signals.take().into_iter().collect();

                let sweep_due = last_sweep.is_none_or(|at| at.elapsed() >= SWEEP_INTERVAL);

                if self.sweeps && sweep_due && self.ownership.is_primary().await {
                    last_sweep = Some(tokio::time::Instant::now());

                    match self.status_pages.dependency_organizations().await {
                        Ok(all) => organizations.extend(all),
                        Err(error) => {
                            tracing::warn!(error = %error, "organizations with component dependencies could not be listed");
                        }
                    }
                }

                if !organizations.is_empty() {
                    self.evaluate(organizations).await;
                }
            }
        })
    }

    #[tracing::instrument(
        name = "worker.status_page_dependencies",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "status_page_dependencies")
    )]
    async fn evaluate(&self, organizations: HashSet<Id>) {
        for org_id in organizations {
            match self
                .status_pages
                .evaluate_component_dependencies(&org_id)
                .await
            {
                Ok(changed) if changed > 0 => {
                    tracing::info!(org_id = %org_id, changed, "component dependency health changed");
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        org_id = %org_id,
                        error = %error,
                        "component dependency evaluation failed"
                    );
                }
            }
        }
    }
}
