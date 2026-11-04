// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 保存规则时按种类多做的几件事：组合规则组合的规则说得通，实时规则的汇总集群在联邦里，心跳规则
//! 头一回保存时给出打卡地址。

use serde_json::Value;

use super::heartbeats;
use crate::{
    api::AppState,
    domain::alerting::rule::{AlertQuery, AlertRule, AlertRuleKind},
    shared::{Error, Result, ids::Id},
};

/// 组合规则组合的规则都在本组织、本身不是组合规则；组合规则自己也不能被别的组合规则组合。这样不会成环。
pub(super) async fn check_composite(
    state: &AppState,
    org_id: &Id,
    kind: AlertRuleKind,
    query: &AlertQuery,
    own_id: Option<&Id>,
) -> Result<()> {
    let rules = state.alerting.service.list_rules(org_id).await?;

    if let Some(own_id) = own_id
        && kind == AlertRuleKind::Composite
        && let Some(parent) = rules.iter().find(|rule| {
            rule.query
                .composite
                .as_ref()
                .is_some_and(|composite| composite.rules.values().any(|id| id == own_id))
        })
    {
        return Err(Error::invalid(format!(
            "this rule is combined by the composite rule `{}`, so it cannot combine rules itself",
            parent.name
        )));
    }

    let Some(composite) = &query.composite else {
        return Ok(());
    };

    for (letter, id) in &composite.rules {
        if own_id == Some(id) {
            return Err(Error::invalid("a composite rule cannot combine itself"));
        }

        let Some(rule) = rules.iter().find(|rule| &rule.id == id) else {
            return Err(Error::invalid(format!(
                "rule {letter} (`{id}`) does not exist in this organization"
            )));
        };

        if rule.kind == AlertRuleKind::Composite {
            return Err(Error::invalid(format!(
                "rule {letter} (`{}`) is a composite rule; a composite rule combines other kinds",
                rule.name
            )));
        }
    }

    Ok(())
}

/// 实时规则的汇总集群是本集群或启用的远端集群。没开联邦时（可选的集群为空）它不起作用，不必校验。
pub(super) async fn check_home_cluster(state: &AppState, query: &AlertQuery) -> Result<()> {
    let Some(home) = query.home_cluster.as_deref() else {
        return Ok(());
    };

    let clusters = state.alerting.sources.clusters().await?;

    if !clusters.is_empty() && !clusters.iter().any(|choice| choice.id == home) {
        return Err(Error::invalid(format!(
            "query.home_cluster `{home}` is neither this cluster nor an enabled remote cluster"
        )));
    }

    Ok(())
}

/// 规则的保存结果；心跳规则头一回有打卡地址时一并给出（只这一次）。
pub(super) async fn saved_rule_json(state: &AppState, saved: &AlertRule) -> Result<Value> {
    let mut body = serde_json::to_value(saved).unwrap_or(Value::Null);

    if saved.kind != AlertRuleKind::Heartbeat
        || state.alerting.heartbeats.get(&saved.id).await?.is_some()
    {
        return Ok(body);
    }

    let address =
        heartbeats::issue_address(state, state.alerting.heartbeats.as_ref(), saved).await?;

    if let Some(object) = body.as_object_mut() {
        object.insert("heartbeatAddress".into(), address);
    }

    Ok(body)
}
