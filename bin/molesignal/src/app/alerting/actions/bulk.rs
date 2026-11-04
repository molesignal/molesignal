// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 批量处理：对几个事件做同一件事，一个失败不影响其余的。

use serde::{Deserialize, Serialize};

use super::{Actor, IncidentActions, Reassignment};
use crate::{
    domain::alerting::{incident::Severity, priority::Priority, urgency::Urgency},
    shared::{Error, ids::Id},
};

/// 一次最多处理几个事件。
pub const MAX_BULK_INCIDENTS: usize = 100;

/// 批量做的事。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum BulkAction {
    Acknowledge,
    Resolve,
    Snooze {
        duration_secs: u32,
    },
    Reassign {
        to: Reassignment,
    },
    SetSeverity {
        severity: Severity,
    },
    SetPriority {
        #[serde(default)]
        priority: Option<Priority>,
    },
    SetUrgency {
        urgency: Urgency,
    },
}

/// 一个事件的结果：成功时没有 `error`。
#[derive(Debug, Clone, Serialize)]
pub struct BulkResult {
    pub id: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IncidentActions {
    /// 对这几个事件逐个做 `action`。事件数在接口层限住（[`MAX_BULK_INCIDENTS`]）。
    pub async fn bulk(&self, actor: &Actor, ids: &[Id], action: &BulkAction) -> Vec<BulkResult> {
        let mut results = Vec::with_capacity(ids.len());

        for id in ids {
            let done = match action {
                BulkAction::Acknowledge => self
                    .service
                    .acknowledge(id, actor.user_id.clone(), actor.at, actor.via)
                    .await
                    .map(drop),
                BulkAction::Resolve => self
                    .service
                    .resolve(id, actor.user_id.clone(), actor.at, actor.via)
                    .await
                    .map(drop),
                BulkAction::Snooze { duration_secs } => {
                    self.snooze(actor, id, *duration_secs).await.map(drop)
                }
                BulkAction::Reassign { to } => self.reassign(actor, id, to.clone()).await.map(drop),
                BulkAction::SetSeverity { severity } => {
                    self.set_severity(actor, id, *severity).await.map(drop)
                }
                BulkAction::SetPriority { priority } => {
                    self.set_priority(actor, id, *priority).await.map(drop)
                }
                BulkAction::SetUrgency { urgency } => {
                    self.set_urgency(actor, id, urgency.clone()).await.map(drop)
                }
            };

            results.push(BulkResult {
                id: id.clone(),
                error: done.err().map(|error: Error| error.to_string()),
            });
        }

        results
    }
}
