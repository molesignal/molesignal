// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 事件处理后的后续：到点验证修复动作，同步关联的工单；工单完结、连接器又开了「工单完结时解决事件」
//! 时解决事件。每分钟一轮，几个告警节点里只由拥有这项工作的那个做。

use std::{sync::Arc, time::Duration};

use tokio::{task::JoinHandle, time::MissedTickBehavior};

use super::{RuleEvaluator, remediation::RemediationService, tickets::TicketService};
use crate::{app::cluster::WorkOwnership, shared::time::TimestampMicros};

/// 归属里这项工作的名字。
const WORK: &str = "alert_followups";

const INTERVAL: Duration = Duration::from_secs(60);

pub fn spawn(
    remediation: Arc<RemediationService>,
    tickets: Arc<TicketService>,
    evaluator: Arc<RuleEvaluator>,
    ownership: Arc<WorkOwnership>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            interval.tick().await;

            if !ownership.owns(WORK).await {
                continue;
            }

            let now = TimestampMicros::now();

            if let Err(error) = remediation.verify_due(now).await {
                tracing::warn!(%error, "remediation verification round failed");
            }

            match tickets.sync_due(now).await {
                Ok(finished) => {
                    for incident_id in finished {
                        if let Err(error) = evaluator.close_from_ticket(&incident_id, now).await {
                            tracing::warn!(%incident_id, %error, "incident could not be resolved after its ticket was done");
                        }
                    }
                }
                Err(error) => tracing::warn!(%error, "ticket sync round failed"),
            }
        }
    })
}
