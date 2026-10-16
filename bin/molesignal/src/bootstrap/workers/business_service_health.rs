// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashSet, sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    app::{
        business_service::BusinessServiceService,
        cluster::{PeerRole, WorkOwnership},
    },
    shared::ids::Id,
};

/// 收到变化后最多等这么久再评估，把同一阵变化合并成一次。
const COALESCE: Duration = Duration::from_secs(2);

/// alert 主节点多久把全部组织评估一遍，补上本节点没收到的变化。
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// 业务服务健康评估：告警事件或拨测状态变化后，评估所在组织的业务服务，并把变化交给关联的状态页
/// 组件。每个节点评估自己收到的变化；alert 主节点（见 [`WorkOwnership::is_primary`]）另外定期
/// 全量评估。状态以比较后写入，多个节点同时评估也只记一次变化。
pub struct BusinessServiceHealthWorker {
    service: Arc<BusinessServiceService>,
    /// 默认不分担（本节点就是主节点）。
    ownership: Arc<WorkOwnership>,
    /// 本节点是否承担 alert 角色、参与全量评估。
    sweeps: bool,
}

impl BusinessServiceHealthWorker {
    pub fn new(service: Arc<BusinessServiceService>) -> Self {
        Self {
            service,
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
                self.service.signals().wait(COALESCE).await;
                tokio::time::sleep(COALESCE).await;
                let mut organizations: HashSet<Id> =
                    self.service.signals().take().into_iter().collect();

                let sweep_due = last_sweep.is_none_or(|at| at.elapsed() >= SWEEP_INTERVAL);

                if self.sweeps && sweep_due && self.ownership.is_primary().await {
                    last_sweep = Some(tokio::time::Instant::now());

                    match self.service.organizations().await {
                        Ok(all) => organizations.extend(all),
                        Err(error) => {
                            tracing::warn!(error = %error, "business service organizations could not be listed");
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
        name = "worker.business_service_health",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "business_service_health")
    )]
    async fn evaluate(&self, organizations: HashSet<Id>) {
        for org_id in organizations {
            match self.service.evaluate_organization(&org_id).await {
                Ok(changed) if changed > 0 => {
                    tracing::info!(org_id = %org_id, changed, "business service health changed");
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        org_id = %org_id,
                        error = %error,
                        "business service health evaluation failed"
                    );
                }
            }
        }
    }
}
