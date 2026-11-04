// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一级里谁都收不到告警时不等确认超时，立即升到下一级：没人值班，或这一级的负责人都没设好通知方式、
//! 所在团队和组织也没有发到群里的默认路由。点了团队的一级不算，团队的默认路由可能发到群里。
//! 每一轮调度里，同一个人只查一次。

use std::collections::HashMap;

use serde_json::json;

use super::EscalationDispatcher;
use crate::{
    app::notify::Reachability,
    domain::alerting::{
        escalation::{EscalationStep, EscalationTarget},
        incident::Incident,
        timeline::TimelineKind,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 一轮调度里查过的人。
pub(super) type ReachCache = HashMap<Id, Reachability>;

impl EscalationDispatcher {
    /// 这一级谁都收不到告警。查不出来时当作收得到，照常等确认超时。
    pub(super) async fn step_unreachable(
        &self,
        incident: &Incident,
        step: &EscalationStep,
        cache: &mut ReachCache,
    ) -> bool {
        if step
            .targets
            .iter()
            .any(|target| matches!(target, EscalationTarget::Team { .. }))
        {
            return false;
        }

        let unknown: Vec<Id> = incident
            .assignees
            .iter()
            .filter(|user_id| !cache.contains_key(*user_id))
            .cloned()
            .collect();

        if !unknown.is_empty() {
            match self
                .notify_engine
                .reachability(&incident.org_id, &unknown)
                .await
            {
                Ok(answers) => cache.extend(answers),
                Err(error) => {
                    tracing::warn!(incident_id = %incident.id, %error, "reachability unreadable; the step waits as usual");
                    return false;
                }
            }
        }

        incident
            .assignees
            .iter()
            .all(|user_id| cache.get(user_id) == Some(&Reachability::None))
    }

    /// 时间线记一笔：第几级（从 0 数，同升级记录）谁都收不到，直接升了。
    pub(super) async fn record_unreachable(
        &self,
        incident: &Incident,
        step_index: usize,
        now: TimestampMicros,
    ) {
        let detail = json!({
            "step": step_index,
            "userIds": incident.assignees,
        });

        self.record(incident, TimelineKind::StepUnreachable, detail, now)
            .await;
    }
}
