// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 拨测事件进来时的样子（[`synthetic_incident`]）：开事件和重定严重度都从它出发，告警流水线两次
//! 看到的是同样的事件。

use std::collections::BTreeMap;

use super::synthetic::{synthetic_fingerprint, synthetic_no_data_fingerprint};
use crate::{
    domain::{
        alerting::incident::{
            CONDITION_LABEL, DESCRIPTION_ANNOTATION, Incident, IncidentStatus, NO_DATA_CONDITION,
            SOURCE_LABEL, SYNTHETIC_SOURCE, SYNTHETIC_STATE_LABEL, Severity,
        },
        synthetics::{MonitorRevision, MonitorState, SyntheticMonitor},
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 拨测事件因何而开。
pub(super) enum Opening<'a> {
    /// 监控项的状态要告警；`result_id` 是触发它的那次结果。
    State {
        state: MonitorState,
        severity: Severity,
        result_id: Option<&'a Id>,
    },
    /// 监控项没有结果；`description` 写明哪里、为什么。
    NoData { description: Option<String> },
}

/// 拨测事件进来时的样子：标题是监控项的名字，标签说明来源、类型、状态与团队。状态事件的注解记着触发它的
/// 那次结果；无数据事件是 warning，另带 `alert.condition = no_data`，注解写明哪里、为什么没有
/// 结果。
pub(super) fn synthetic_incident(
    monitor: &SyntheticMonitor,
    revision: &MonitorRevision,
    opening: Opening<'_>,
    now: TimestampMicros,
) -> Incident {
    let state = match &opening {
        Opening::State { state, .. } => state.as_str(),
        Opening::NoData { .. } => NO_DATA_CONDITION,
    };

    let mut labels = BTreeMap::from([
        (SOURCE_LABEL.to_string(), SYNTHETIC_SOURCE.to_string()),
        (
            "synthetic.kind".to_string(),
            monitor.kind.as_str().to_string(),
        ),
        (SYNTHETIC_STATE_LABEL.to_string(), state.to_string()),
    ]);

    if let Some(team_id) = &monitor.team_id {
        labels.insert("team_id".into(), team_id.to_string());
    }

    if !monitor.tags.is_empty() {
        labels.insert("synthetic.tags".into(), monitor.tags.join(","));
    }

    let mut annotations = BTreeMap::new();

    let (severity, fingerprint) = match opening {
        Opening::State {
            severity,
            result_id,
            ..
        } => {
            if let Some(result_id) = result_id {
                annotations.insert("synthetic.result_id".to_string(), result_id.to_string());
            }

            (severity, synthetic_fingerprint(&monitor.id))
        }
        Opening::NoData { description } => {
            labels.insert(CONDITION_LABEL.into(), NO_DATA_CONDITION.into());

            if let Some(description) = description {
                annotations.insert(DESCRIPTION_ANNOTATION.to_string(), description);
            }

            (
                Severity::Warning,
                synthetic_no_data_fingerprint(&monitor.id),
            )
        }
    };

    Incident {
        id: Id::new(),
        org_id: monitor.organization_id.clone(),
        rule_id: monitor.id.clone(),
        // 不关联升级策略：找不到策略的事件没有可升级的对象，只按通知策略通知。
        escalation_policy_id: revision
            .escalation_policy_id
            .clone()
            .unwrap_or_else(|| Id::from_string("")),
        status: IncidentStatus::Open,
        severity,
        summary: monitor.name.clone(),
        fingerprint,
        current_step: 0,
        current_loop: 0,
        current_step_started_at: now,
        assignees: Vec::new(),
        labels,
        annotations,
        trace_ids: Vec::new(),
        host_ids: Vec::new(),
        affected_services: Vec::new(),
        triggering_query: None,
        created_at: now,
        acknowledged_at: None,
        acknowledged_by: None,
        resolved_at: None,
        resolved_by: None,
        team_id: None,
        routing: None,
        handling: Default::default(),
        revision: 0,
    }
}
