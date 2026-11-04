// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 事件触发前后的变更：事件涉及的服务及其下游上报的部署、配置和开关变更（触发前 [`BEFORE`] 到触发后
//! [`AFTER`]），以及触发前 [`RULE_EDIT_WINDOW`] 里改过的这条规则。

use std::collections::HashSet;

use serde::Serialize;

use crate::{
    domain::{
        alerting::{incident::Incident, rule::AlertRule},
        apm::{ChangeKind, DeploymentEvent, DeploymentStatus},
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 变更最早可以早于触发多久，最晚可以晚于触发多久（指标分桶有延迟）。与 APM 洞察的取法一致。
pub const BEFORE: i64 = 30 * 60_000_000;
pub const AFTER: i64 = 10 * 60_000_000;
/// 规则在触发前多久内改过就列出来。
pub const RULE_EDIT_WINDOW: i64 = 24 * 3_600_000_000;
/// 最多列几条。
const MOST_CHANGES: usize = 20;

/// 一条变更。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentChange {
    /// `deployment`、`configuration`、`feature_flag`，或 `rule`（这条规则被改过）。
    pub kind: &'static str,
    pub at: TimestampMicros,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// 下游服务的变更，不是事件涉及的服务自己的。
    pub downstream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<DeploymentStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 谁发起的变更（流水线上报的作者）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// 流水线或合并请求的地址。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IncidentChanges {
    pub from: TimestampMicros,
    pub to: TimestampMicros,
    pub changes: Vec<IncidentChange>,
}

/// 事件涉及的服务：受影响的服务和 `service` 标签。`命名空间/服务` 的写法只取服务名。
pub fn incident_services(incident: &Incident) -> HashSet<String> {
    incident
        .affected_services
        .iter()
        .chain(incident.labels.get("service"))
        .map(|service| bare(service).to_string())
        .filter(|service| !service.is_empty())
        .collect()
}

/// 从上报的变更和规则里挑出这个事件的，按时间先后。
pub fn select_changes(
    incident: &Incident,
    reported: &[DeploymentEvent],
    downstream: &[String],
    rule: Option<&AlertRule>,
) -> IncidentChanges {
    let triggered = incident.created_at.0;
    let from = triggered - BEFORE;
    let to = triggered + AFTER;

    let own = incident_services(incident);
    let below: HashSet<String> = downstream
        .iter()
        .map(|service| bare(service).to_string())
        .filter(|service| !own.contains(service))
        .collect();

    let mut changes: Vec<IncidentChange> = reported
        .iter()
        .filter(|event| event.deployed_at.0 >= from && event.deployed_at.0 <= to)
        .filter_map(|event| {
            let name = bare(&event.service);
            let downstream = if own.contains(name) {
                false
            } else if below.contains(name) {
                true
            } else {
                return None;
            };

            Some(IncidentChange {
                kind: match event.kind {
                    ChangeKind::Deployment => "deployment",
                    ChangeKind::Configuration => "configuration",
                    ChangeKind::FeatureFlag => "feature_flag",
                },
                at: event.deployed_at,
                service: Some(event.service.clone()),
                downstream,
                version: Some(event.version.clone()).filter(|version| !version.is_empty()),
                status: Some(event.status),
                commit: event.commit.clone(),
                description: event.description.clone(),
                author: event.author.clone(),
                link: event
                    .ci_url
                    .clone()
                    .or_else(|| event.pull_request_url.clone()),
                rule_id: None,
                rule_name: None,
            })
        })
        .collect();

    if let Some(rule) = rule
        && rule.updated_at.0 >= triggered - RULE_EDIT_WINDOW
        && rule.updated_at.0 <= triggered
        && rule.updated_at.0 > rule.created_at.0
    {
        changes.push(IncidentChange {
            kind: "rule",
            at: rule.updated_at,
            service: None,
            downstream: false,
            version: None,
            status: None,
            commit: None,
            description: None,
            author: None,
            link: None,
            rule_id: Some(rule.id.clone()),
            rule_name: Some(rule.name.clone()),
        });
    }

    changes.sort_by_key(|change| change.at.0);
    changes.truncate(MOST_CHANGES);

    IncidentChanges {
        from: TimestampMicros(from),
        to: TimestampMicros(to),
        changes,
    }
}

fn bare(service: &str) -> &str {
    service.rsplit('/').next().unwrap_or(service).trim()
}

#[cfg(test)]
#[path = "changes_tests.rs"]
mod tests;
