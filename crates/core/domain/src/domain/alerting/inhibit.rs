// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警抑制：有更严重（或更上游）的事件在时，压住次要事件的通知。
//!
//! 和静默一样只压通知和升级：事件照开、照记，页面上看得见，状态页和工单不受影响。一条规则说：
//! 符合「源」条件的事件在，同时符合「目标」条件、`equal` 里的标签取值都相同的事件就被压住。
//! `more_severe_only` 开着（默认）时源事件必须严格比目标事件严重，一条规则就能写出「同一个服务上，
//! 更严重的压住次要的」。
//!
//! 怎么算（[`resolve`]）保证一件事：**被抑制的事件，上游总有一个真的会通知人的事件。**
//! - 被静默的事件不当源：它自己没人收到，再压别的就谁都收不到了。
//! - 已确认的事件照样当源：有人在处理了。
//! - 并入别的事件的两边都不算；有人手动开的、有人说了照常通知的不当目标。
//! - 互相压的（A 压 B、B 压 A）按严重度、先后留下一个不压。
//!
//! 时间上的两个缓冲都写在规则上：`wait_secs` 目标事件先等多久（症状常常比根因先报出来），
//! `linger_secs` 源事件恢复后再压多久（根因恢复时症状也快恢复了，不必再叫一遍人）。

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    incident::{Incident, MANUAL_SOURCE, SOURCE_LABEL, Severity},
    mute::INCIDENT_ID_MATCHER_LABEL,
    semantic_group::LabelMatcher,
};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

mod resolve;
mod settle;

pub use resolve::{InhibitPair, Inhibited, active_ids, inhibiting_counts, preview, resolve};
pub use settle::{InhibitionChange, settle};

/// 规则名最多几个字符。
pub const MAX_RULE_NAME_CHARS: usize = 255;
/// 一边最多几个标签条件。
pub const MAX_MATCHERS: usize = 32;
/// `equal` 最多几个标签。
pub const MAX_EQUAL_LABELS: usize = 16;
/// 目标事件默认先等多久：规则大多一分钟评估一次，根因可能比症状晚一轮报出来。
pub const DEFAULT_WAIT_SECS: u32 = 60;
/// 目标事件最多先等多久。
pub const MAX_WAIT_SECS: u32 = 600;
/// 源事件恢复后默认再压多久。
pub const DEFAULT_LINGER_SECS: u32 = 120;
/// 源事件恢复后最多再压多久。
pub const MAX_LINGER_SECS: u32 = 3600;
/// 目标事件最短也等这么久：抑制由升级派发的下一轮（默认 10 秒）判定，第一条通知不能抢在它前面。
pub const SETTLE_SECS: u32 = 15;

fn more_severe_only_by_default() -> bool {
    true
}

fn default_wait_secs() -> u32 {
    DEFAULT_WAIT_SECS
}

fn default_linger_secs() -> u32 {
    DEFAULT_LINGER_SECS
}

/// 哪些事件能压别的。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InhibitSource {
    /// 全部命中才算；标签还认 `service`、`host`（事件没带这个标签时取受影响的服务、主机）和
    /// `alert.rule_id`（开事件的规则或来源）。
    #[serde(default)]
    pub matchers: Vec<LabelMatcher>,
    /// 严重度不低于它才算；空着不限。
    #[serde(default)]
    pub min_severity: Option<Severity>,
}

/// 哪些事件会被压。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InhibitTarget {
    /// 全部命中才算；空着是所有事件。
    #[serde(default)]
    pub matchers: Vec<LabelMatcher>,
    /// 严重度不高于它才算；空着不限。
    #[serde(default)]
    pub max_severity: Option<Severity>,
}

/// 一条抑制规则。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InhibitRule {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub enabled: bool,
    pub source: InhibitSource,
    pub target: InhibitTarget,
    /// 这些标签两边都要有、取值相同才压；空着不看标签（这时 `source.matchers` 不能空）。
    #[serde(default)]
    pub equal: Vec<String>,
    /// 源事件必须严格比目标事件严重。
    #[serde(default = "more_severe_only_by_default")]
    pub more_severe_only: bool,
    /// 目标事件的通知先等多久，看源事件会不会来。
    #[serde(default = "default_wait_secs")]
    pub wait_secs: u32,
    /// 源事件恢复后再压多久。
    #[serde(default = "default_linger_secs")]
    pub linger_secs: u32,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl InhibitRule {
    /// 收拾名称和 `equal`，再看规则说不说得通。
    pub fn validate(&mut self) -> Result<()> {
        self.name = self.name.trim().to_string();

        if self.name.is_empty() {
            return Err(Error::invalid("name cannot be empty"));
        }

        if self.name.chars().count() > MAX_RULE_NAME_CHARS {
            return Err(Error::invalid(format!(
                "name has at most {MAX_RULE_NAME_CHARS} characters"
            )));
        }

        for label in &mut self.equal {
            *label = label.trim().to_string();
        }

        self.equal.retain(|label| !label.is_empty());
        self.equal.sort();
        self.equal.dedup();

        if self.equal.len() > MAX_EQUAL_LABELS {
            return Err(Error::invalid(format!(
                "equal has at most {MAX_EQUAL_LABELS} labels"
            )));
        }

        validate_matchers(&self.source.matchers)?;
        validate_matchers(&self.target.matchers)?;

        // 什么事件都能当源、又不看标签：随便一个事件就压住全组织。
        if self.source.matchers.is_empty() && self.equal.is_empty() {
            return Err(Error::invalid(
                "name which incidents suppress others, or the labels they must share: \
                 otherwise any incident would suppress every other",
            ));
        }

        if self.wait_secs > MAX_WAIT_SECS {
            return Err(Error::invalid(format!(
                "wait_secs is at most {MAX_WAIT_SECS}"
            )));
        }

        if self.linger_secs > MAX_LINGER_SECS {
            return Err(Error::invalid(format!(
                "linger_secs is at most {MAX_LINGER_SECS}"
            )));
        }

        Ok(())
    }

    /// `equal` 里的标签在这个事件上的取值，缺一个就是 `None`（缺的不算相同）。
    fn class(&self, view: &BTreeMap<String, String>) -> Option<Vec<String>> {
        self.equal
            .iter()
            .map(|label| {
                view.get(label)
                    .filter(|value| !value.trim().is_empty())
                    .cloned()
            })
            .collect()
    }

    /// 有 `source` 这个严重度的事件能不能压有 `target` 这个严重度的事件。
    fn severities_allow(&self, source: Severity, target: Severity) -> bool {
        !self.more_severe_only || source > target
    }
}

fn validate_matchers(matchers: &[LabelMatcher]) -> Result<()> {
    if matchers.len() > MAX_MATCHERS {
        return Err(Error::invalid(format!(
            "a side has at most {MAX_MATCHERS} label conditions"
        )));
    }

    for matcher in matchers {
        if matcher.label.trim().is_empty() {
            return Err(Error::invalid("a label condition needs a label"));
        }

        if matcher.label == INCIDENT_ID_MATCHER_LABEL {
            return Err(Error::invalid(format!(
                "label `{}` is kept for MoleSignal",
                matcher.label
            )));
        }

        if !matcher.regex_is_valid() {
            return Err(Error::invalid(format!(
                "the regular expression of label `{}` does not parse",
                matcher.label
            )));
        }
    }

    Ok(())
}

/// 规则判断用的标签：事件的标签，加上受影响的服务和主机（事件自己没带这两个标签时），以及开事件的
/// 规则或来源。
fn match_view(incident: &Incident) -> BTreeMap<String, String> {
    let mut view = incident.labels.clone();

    if let Some(service) = incident.affected_services.first() {
        view.entry("service".into())
            .or_insert_with(|| service.clone());
    }

    if let Some(host) = incident.host_ids.first() {
        view.entry("host".into()).or_insert_with(|| host.clone());
    }

    view.insert("alert.rule_id".into(), incident.rule_id.to_string());
    view
}

/// 这个事件能不能被压：有人手动开的、有人说了照常通知的不能。
fn may_be_target(incident: &Incident) -> bool {
    incident.handling.inhibition_lifted_by.is_none()
        && incident.labels.get(SOURCE_LABEL).map(String::as_str) != Some(MANUAL_SOURCE)
}

/// 新开的事件要先等到什么时候：它符合某条启用规则的目标条件、又可能被压时，等规则的 `wait_secs`，
/// 最短 [`SETTLE_SECS`]。不会被任何规则压的事件不等。
pub fn wait_until(rules: &[InhibitRule], incident: &Incident) -> Option<TimestampMicros> {
    if !may_be_target(incident) {
        return None;
    }

    let view = match_view(incident);

    let wait = rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter(|rule| {
            // 最严重的事件不会有更严重的来压它。
            let can_be_outranked = !rule.more_severe_only || incident.severity < Severity::Critical;

            can_be_outranked
                && rule.class(&view).is_some()
                && rule
                    .target
                    .max_severity
                    .is_none_or(|ceiling| incident.severity <= ceiling)
                && rule.target.matchers.iter().all(|m| m.matches(&view))
        })
        .map(|rule| rule.wait_secs.max(SETTLE_SECS))
        .max()?;

    Some(TimestampMicros(
        incident
            .created_at
            .0
            .saturating_add(i64::from(wait) * 1_000_000),
    ))
}

#[async_trait]
pub trait InhibitRuleRepository: Send + Sync {
    async fn create(&self, rule: InhibitRule) -> Result<InhibitRule>;
    async fn update(&self, rule: InhibitRule) -> Result<InhibitRule>;
    async fn get(&self, id: &Id) -> Result<InhibitRule>;
    async fn list(&self, org_id: &Id) -> Result<Vec<InhibitRule>>;
    async fn delete(&self, id: &Id) -> Result<()>;
    /// 升级派发和开事件时用：这个组织启用的规则。
    async fn list_enabled(&self, org_id: &Id) -> Result<Vec<InhibitRule>>;
}

#[cfg(test)]
mod tests;
