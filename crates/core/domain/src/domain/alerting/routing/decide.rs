// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 分派：给一个事件算出团队和升级策略。纯函数，路由表在内存里编好（正则只编一次）。

use std::collections::{BTreeMap, HashMap, HashSet};

use regex::Regex;
use serde::Serialize;

use super::{
    ConditionOperator, IncidentOrigin, IncidentRouting, LabelCondition, RoutingReason, RoutingRule,
    RoutingTarget, SERVICE_LABELS, SERVICE_NAMESPACE_LABEL, TEAM_ID_LABEL, TEAM_LABEL, TeamPolicy,
};
use crate::{
    domain::{
        alerting::{escalation::DEFAULT_POLICY_ID, incident::Severity, priority::Priority},
        apm::DEFAULT_SERVICE_NAMESPACE,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 正则要匹配整个值。
pub(super) fn anchored(pattern: &str) -> Result<Regex, regex::Error> {
    Regex::new(&format!("^(?:{pattern})$"))
}

/// 一个服务在 APM 服务目录里登记的负责团队。
#[derive(Debug, Clone)]
pub struct ServiceOwner {
    pub namespace: String,
    pub name: String,
    pub team_id: Id,
}

/// 要分派的事件：它的标签、受影响的服务、严重度、优先级、开出来的时刻，以及来源选的升级策略。
pub struct RoutingSubject<'a> {
    pub labels: &'a BTreeMap<String, String>,
    pub affected_services: &'a [String],
    pub severity: Severity,
    pub priority: Option<Priority>,
    /// 事件开出来的时刻：规则的时间条件按它算。
    pub at: TimestampMicros,
    /// 别名 `default` 交给路由；明确的策略或空（不升级）照旧，只定团队。
    pub source_policy: &'a Id,
}

/// 分派的结果，写到事件上。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingDecision {
    pub escalation_policy_id: Id,
    pub team_id: Option<Id>,
    pub routing: IncidentRouting,
}

/// 分派的结果，加上每一步看到了什么：给「试一试」用。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingExplanation {
    pub decision: RoutingDecision,
    pub matched_rule_id: Option<Id>,
    /// 用来查归属的服务：`命名空间/服务名`。
    pub service: Option<String>,
    pub service_team_id: Option<Id>,
    pub label_team_id: Option<Id>,
}

struct CompiledRule {
    rule: RoutingRule,
    /// 与条件一一对应；只有正则条件有。
    patterns: Vec<Option<Regex>>,
}

/// 一个组织的路由表：启用的规则（按位置）、团队的值班策略、服务归属、团队和现有的升级策略。
pub struct RoutingTable {
    rules: Vec<CompiledRule>,
    team_policies: HashMap<Id, Id>,
    service_teams: BTreeMap<(String, String), Id>,
    /// 小写的团队名对应的团队；同名的不止一个时为 `None`，按名字认不出。
    teams_by_name: BTreeMap<String, Option<Id>>,
    team_ids: HashSet<Id>,
    policies: HashSet<Id>,
}

/// 团队从哪来。
#[derive(Clone, Copy, PartialEq, Eq)]
enum TeamFrom {
    Rule,
    Service,
    Label,
}

impl RoutingTable {
    pub fn new(
        mut rules: Vec<RoutingRule>,
        team_policies: Vec<TeamPolicy>,
        owners: Vec<ServiceOwner>,
        teams: Vec<(Id, String)>,
        policies: Vec<Id>,
    ) -> Self {
        rules.retain(|rule| rule.enabled);
        rules.sort_by_key(|rule| rule.position);

        let rules = rules
            .into_iter()
            .filter_map(|rule| {
                // 写入时已经校验过；同步来的规则编不过正则就不参与。
                let patterns = rule
                    .conditions
                    .iter()
                    .map(|condition| match condition.operator {
                        ConditionOperator::Matches => condition
                            .values
                            .first()
                            .map(|pattern| anchored(pattern).map(Some))
                            .unwrap_or(Ok(None)),
                        _ => Ok(None),
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?;

                Some(CompiledRule { rule, patterns })
            })
            .collect();

        let mut teams_by_name: BTreeMap<String, Option<Id>> = BTreeMap::new();

        for (id, name) in &teams {
            teams_by_name
                .entry(name.trim().to_lowercase())
                .and_modify(|found| *found = None)
                .or_insert_with(|| Some(id.clone()));
        }

        Self {
            rules,
            team_policies: team_policies
                .into_iter()
                .map(|policy| (policy.team_id, policy.escalation_policy_id))
                .collect(),
            service_teams: owners
                .into_iter()
                .map(|owner| ((owner.namespace, owner.name), owner.team_id))
                .collect(),
            teams_by_name,
            team_ids: teams.into_iter().map(|(id, _)| id).collect(),
            policies: policies.into_iter().collect(),
        }
    }

    pub fn decide(&self, subject: &RoutingSubject<'_>) -> RoutingDecision {
        self.explain(subject).decision
    }

    pub fn explain(&self, subject: &RoutingSubject<'_>) -> RoutingExplanation {
        let service = service_of(subject);
        let service_team = service
            .as_ref()
            .and_then(|key| self.service_teams.get(key))
            .cloned();
        let label_team = self.label_team(subject.labels);
        let routed = subject.source_policy.0 == DEFAULT_POLICY_ID;
        let matched = routed.then(|| self.first_match(subject)).flatten();

        let rule_team = matched.and_then(|rule| match &rule.target {
            RoutingTarget::Team { team_id } => Some(team_id.clone()),
            RoutingTarget::Policy { .. } => None,
        });

        let (team, team_from) = match (&rule_team, &service_team, &label_team) {
            (Some(team), _, _) => (Some(team.clone()), Some(TeamFrom::Rule)),
            (None, Some(team), _) => (Some(team.clone()), Some(TeamFrom::Service)),
            (None, None, Some(team)) => (Some(team.clone()), Some(TeamFrom::Label)),
            (None, None, None) => (None, None),
        };

        let (escalation_policy_id, reason) = if !routed {
            (subject.source_policy.clone(), RoutingReason::Source)
        } else if let Some(rule) = matched {
            let policy = match &rule.target {
                RoutingTarget::Policy { policy_id } => policy_id.clone(),
                RoutingTarget::Team { team_id } => self.policy_of(team_id),
            };

            (policy, RoutingReason::Rule)
        } else {
            match (team_from, &team) {
                (Some(TeamFrom::Service), Some(team)) => {
                    (self.policy_of(team), RoutingReason::ServiceOwner)
                }
                (Some(TeamFrom::Label), Some(team)) => {
                    (self.policy_of(team), RoutingReason::TeamLabel)
                }
                _ => (default_alias(), RoutingReason::Default),
            }
        };

        let service_name = service.map(|(namespace, name)| format!("{namespace}/{name}"));

        let routing = IncidentRouting {
            reason,
            rule_id: matched.map(|rule| rule.id.clone()),
            rule_name: matched.map(|rule| rule.name.clone()),
            service: (team_from == Some(TeamFrom::Service))
                .then(|| service_name.clone())
                .flatten(),
        };

        RoutingExplanation {
            decision: RoutingDecision {
                escalation_policy_id,
                team_id: team,
                routing,
            },
            matched_rule_id: matched.map(|rule| rule.id.clone()),
            service: service_name,
            service_team_id: service_team,
            label_team_id: label_team,
        }
    }

    /// 团队的值班策略；没设时走默认策略。
    fn policy_of(&self, team_id: &Id) -> Id {
        self.team_policies
            .get(team_id)
            .cloned()
            .unwrap_or_else(default_alias)
    }

    fn first_match(&self, subject: &RoutingSubject<'_>) -> Option<&RoutingRule> {
        let origin = IncidentOrigin::of(subject.labels);

        self.rules
            .iter()
            .find(|compiled| {
                let rule = &compiled.rule;

                let target_exists = match &rule.target {
                    RoutingTarget::Policy { policy_id } => self.policies.contains(policy_id),
                    RoutingTarget::Team { team_id } => self.team_ids.contains(team_id),
                };

                target_exists
                    && (rule.origins.is_empty() || rule.origins.contains(&origin))
                    && rule
                        .min_severity
                        .is_none_or(|minimum| subject.severity >= minimum)
                    && (rule.priorities.is_empty()
                        || subject
                            .priority
                            .is_some_and(|priority| rule.priorities.contains(&priority)))
                    && rule.time.as_ref().is_none_or(|time| time.holds(subject.at))
                    && rule
                        .conditions
                        .iter()
                        .zip(&compiled.patterns)
                        .all(|(condition, pattern)| {
                            holds(condition, pattern.as_ref(), subject.labels)
                        })
            })
            .map(|compiled| &compiled.rule)
    }

    /// 标签 `team_id` 是现有团队的 id 时用它，否则按 `team` 的名字认（同名的不止一个时认不出）。
    fn label_team(&self, labels: &BTreeMap<String, String>) -> Option<Id> {
        if let Some(id) = labels
            .get(TEAM_ID_LABEL)
            .map(|value| Id::from_string(value.trim()))
            && self.team_ids.contains(&id)
        {
            return Some(id);
        }

        let name = labels.get(TEAM_LABEL)?.trim().to_lowercase();

        self.teams_by_name.get(&name).cloned().flatten()
    }
}

pub(super) fn holds(
    condition: &LabelCondition,
    pattern: Option<&Regex>,
    labels: &BTreeMap<String, String>,
) -> bool {
    let value = labels.get(condition.key.trim());
    let first = condition.values.first();

    match condition.operator {
        ConditionOperator::Equals => value.is_some() && value == first,
        ConditionOperator::NotEquals => value != first,
        ConditionOperator::In => value.is_some_and(|value| condition.values.contains(value)),
        ConditionOperator::Matches => value
            .zip(pattern)
            .is_some_and(|(value, pattern)| pattern.is_match(value)),
    }
}

/// 事件的服务：`(命名空间, 服务名)`。
fn service_of(subject: &RoutingSubject<'_>) -> Option<(String, String)> {
    let name = SERVICE_LABELS
        .iter()
        .filter_map(|key| subject.labels.get(*key))
        .chain(subject.affected_services.iter())
        .map(|value| value.trim())
        .find(|value| !value.is_empty())?;

    let namespace = subject
        .labels
        .get(SERVICE_NAMESPACE_LABEL)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_SERVICE_NAMESPACE);

    Some((namespace.to_string(), name.to_string()))
}

fn default_alias() -> Id {
    Id::from_string(DEFAULT_POLICY_ID)
}

#[cfg(test)]
mod tests;
