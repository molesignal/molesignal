// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 人的响应：多久有人接手（MTTA）、多久恢复（MTTR）、升级率、各级的确认率和确认途径，按团队、服务、
//! 个人拆分。全从事件本身和它的时间线算出来。
//!
//! - **接手**：第一次确认；没确认就被人解决的，算解决的那一刻。时间线早于它出现的事件没有途径，记作
//!   `web`。
//! - **升级过**：到过第 2 级，或被人立即升级。
//! - **各级确认率**：到过这一级的事件里，在这一级被确认的比例；只算时间线上有升级记录的事件。

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde_json::Value;

use crate::{
    domain::alerting::{
        incident::Incident,
        timeline::{TimelineEntry, TimelineKind},
    },
    shared::ids::Id,
};

/// 各拆分维度最多列多少项。
const MOST_ROWS: usize = 20;

/// 一个事件和响应有关的几件事。
#[derive(Debug, Clone, PartialEq)]
pub struct IncidentFacts {
    /// 第一次有人接手。
    pub response: Option<FirstResponse>,
    /// 从触发到恢复多久（微秒）；还没恢复的没有。
    pub resolved_after: Option<i64>,
    pub escalated: bool,
    /// 时间线上到过的各级（从 0 起），没有升级记录时为空。
    pub reached: Vec<u32>,
    /// 在哪一级被确认。
    pub acknowledged_at_step: Option<u32>,
    pub team_id: Option<Id>,
    pub services: Vec<String>,
    /// `P1`…`P5`；没设优先级的事件为空。
    pub priority: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FirstResponse {
    /// 从触发起多久（微秒）。
    pub after: i64,
    pub by: Option<Id>,
    /// `web`、`api`、`im`、`sms`、`phone` 或 `agent`。
    pub via: String,
}

/// 一组事件的响应。时长都是秒。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseSummary {
    pub incidents: usize,
    pub responded: usize,
    pub mtta: Durations,
    pub mttr: Durations,
    /// 0..1。
    pub escalation_rate: f64,
    pub steps: Vec<StepRate>,
    pub ack_via: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Durations {
    pub mean_secs: f64,
    pub p50_secs: f64,
    pub p90_secs: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepRate {
    /// 从 0 起。
    pub step: u32,
    pub reached: u32,
    pub acknowledged: u32,
}

/// 按团队、服务、个人拆开的一行。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakdownRow {
    pub key: String,
    pub incidents: usize,
    pub mtta_secs: f64,
    pub mttr_secs: f64,
    pub escalation_rate: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Team,
    Service,
    /// 第一个接手的人。
    Person,
    /// 优先级；没设的不算在里面。
    Priority,
}

/// 从事件和它的时间线（按时间先后，只要确认、解决、升级三种）读出响应的几件事。
pub fn incident_facts(incident: &Incident, entries: &[TimelineEntry]) -> IncidentFacts {
    let created = incident.created_at.0;

    let escalations: Vec<(i64, u32, String)> = entries
        .iter()
        .filter(|entry| entry.kind == TimelineKind::Escalated)
        .map(|entry| {
            (
                entry.at.0,
                step_of(&entry.detail),
                text(&entry.detail, "reason"),
            )
        })
        .collect();

    let acknowledged = entries
        .iter()
        .find(|entry| entry.kind == TimelineKind::Acknowledged)
        .map(|entry| (entry.at.0, entry.actor.clone(), via_of(&entry.detail)))
        .or_else(|| {
            incident
                .acknowledged_at
                .map(|at| (at.0, incident.acknowledged_by.clone(), "web".to_string()))
        });

    let response = acknowledged
        .clone()
        .or_else(|| {
            let resolved_by = incident.resolved_by.clone()?;
            let resolved_at = incident.resolved_at?;
            let via = entries
                .iter()
                .find(|entry| entry.kind == TimelineKind::Resolved)
                .map(|entry| via_of(&entry.detail))
                .unwrap_or_else(|| "web".into());

            Some((resolved_at.0, Some(resolved_by), via))
        })
        .map(|(at, by, via)| FirstResponse {
            after: (at - created).max(0),
            by,
            via,
        });

    let mut reached: Vec<u32> = escalations.iter().map(|(_, step, _)| *step).collect();
    reached.sort_unstable();
    reached.dedup();

    let acknowledged_at_step = acknowledged.as_ref().map(|(at, _, _)| {
        escalations
            .iter()
            .rev()
            .find(|(when, _, _)| when <= at)
            .map(|(_, step, _)| *step)
            .unwrap_or(0)
    });

    IncidentFacts {
        response,
        resolved_after: incident.resolved_at.map(|at| (at.0 - created).max(0)),
        escalated: escalations
            .iter()
            .any(|(_, step, reason)| *step >= 1 || reason == "manual"),
        reached,
        acknowledged_at_step,
        team_id: incident.team_id.clone(),
        services: incident.affected_services.clone(),
        priority: incident
            .handling
            .priority
            .map(|priority| priority.as_str().to_string()),
    }
}

/// 一组事件的响应。
pub fn summarize(facts: &[IncidentFacts]) -> ResponseSummary {
    let responses: Vec<i64> = facts
        .iter()
        .filter_map(|fact| fact.response.as_ref().map(|response| response.after))
        .collect();
    let recoveries: Vec<i64> = facts
        .iter()
        .filter_map(|fact| fact.resolved_after)
        .collect();

    let mut ack_via: BTreeMap<String, u32> = BTreeMap::new();

    for response in facts.iter().filter_map(|fact| fact.response.as_ref()) {
        *ack_via.entry(response.via.clone()).or_insert(0) += 1;
    }

    let mut steps: BTreeMap<u32, StepRate> = BTreeMap::new();

    for fact in facts.iter().filter(|fact| !fact.reached.is_empty()) {
        for step in &fact.reached {
            steps
                .entry(*step)
                .or_insert(StepRate {
                    step: *step,
                    reached: 0,
                    acknowledged: 0,
                })
                .reached += 1;
        }

        if let Some(step) = fact.acknowledged_at_step
            && let Some(rate) = steps.get_mut(&step)
        {
            rate.acknowledged += 1;
        }
    }

    ResponseSummary {
        incidents: facts.len(),
        responded: responses.len(),
        mtta: durations(responses),
        mttr: durations(recoveries),
        escalation_rate: share(
            facts.iter().filter(|fact| fact.escalated).count(),
            facts.len(),
        ),
        steps: steps.into_values().collect(),
        ack_via,
    }
}

/// 按一个维度拆开，事件多的在前，最多 [`MOST_ROWS`] 行。一个事件涉及几个服务时，每个服务各算一次。
pub fn breakdown(facts: &[IncidentFacts], dimension: Dimension) -> Vec<BreakdownRow> {
    let mut groups: HashMap<String, Vec<&IncidentFacts>> = HashMap::new();

    for fact in facts {
        let keys: Vec<String> = match dimension {
            Dimension::Team => fact.team_id.iter().map(|team| team.0.clone()).collect(),
            Dimension::Service => fact.services.clone(),
            Dimension::Person => fact
                .response
                .as_ref()
                .and_then(|response| response.by.as_ref())
                .map(|by| vec![by.0.clone()])
                .unwrap_or_default(),
            Dimension::Priority => fact.priority.iter().cloned().collect(),
        };

        for key in keys {
            groups.entry(key).or_default().push(fact);
        }
    }

    let mut rows: Vec<BreakdownRow> = groups
        .into_iter()
        .map(|(key, members)| {
            let responses: Vec<i64> = members
                .iter()
                .filter_map(|fact| fact.response.as_ref().map(|response| response.after))
                .collect();
            let recoveries: Vec<i64> = members
                .iter()
                .filter_map(|fact| fact.resolved_after)
                .collect();

            BreakdownRow {
                key,
                incidents: members.len(),
                mtta_secs: durations(responses).mean_secs,
                mttr_secs: durations(recoveries).mean_secs,
                escalation_rate: share(
                    members.iter().filter(|fact| fact.escalated).count(),
                    members.len(),
                ),
            }
        })
        .collect();

    rows.sort_by(|a, b| {
        b.incidents
            .cmp(&a.incidents)
            .then_with(|| a.key.cmp(&b.key))
    });
    rows.truncate(MOST_ROWS);
    rows
}

/// 均值和按最近秩取的 P50、P90，微秒换成秒。
fn durations(mut values: Vec<i64>) -> Durations {
    if values.is_empty() {
        return Durations::default();
    }

    values.sort_unstable();

    let secs = |micros: i64| micros as f64 / 1_000_000.0;
    let rank = |q: f64| {
        let index = ((q * values.len() as f64).ceil() as usize).clamp(1, values.len()) - 1;
        secs(values[index])
    };

    Durations {
        mean_secs: secs(values.iter().sum::<i64>()) / values.len() as f64,
        p50_secs: rank(0.5),
        p90_secs: rank(0.9),
    }
}

fn share(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

fn step_of(detail: &Value) -> u32 {
    detail
        .get("step")
        .and_then(Value::as_u64)
        .and_then(|step| u32::try_from(step).ok())
        .unwrap_or(0)
}

fn via_of(detail: &Value) -> String {
    match text(detail, "via") {
        via if via.is_empty() => "web".into(),
        via => via,
    }
}

fn text(detail: &Value, key: &str) -> String {
    detail
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;
