// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 路由：事件开出来时交给哪个团队、走哪条升级策略。
//!
//! 来源（规则、外部告警来源、拨测检查）把升级策略留给默认（别名 `default`）时，按这个顺序定：
//!
//! 1. 路由规则，自上而下，第一条命中的生效；
//! 2. 服务的负责团队（APM 服务目录登记的团队）；
//! 3. 标签 `team`（团队名）或 `team_id` 指向的团队；
//! 4. 默认策略。
//!
//! 团队和升级策略分开定：团队取第一个说得出的（命中规则的团队、服务的负责团队、标签里的团队），
//! 升级策略取命中规则的策略，否则取这个团队的值班策略，都没有就是默认策略。来源明确选了策略
//! （或者选了不升级）时只定团队，策略照旧。结果在开事件的那一刻写在事件上，之后改路由不影响
//! 开着的事件。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    incident::{EXTERNAL_SOURCE, MANUAL_SOURCE, SOURCE_LABEL, SYNTHETIC_SOURCE, Severity},
    priority::Priority,
    time_window::{CalendarUse, TimeCondition},
};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

mod decide;

pub use decide::{RoutingDecision, RoutingExplanation, RoutingSubject, RoutingTable, ServiceOwner};

/// 一个组织最多几条路由规则。
pub const MAX_ROUTING_RULES: usize = 200;
/// 一条规则最多几个条件。
pub const MAX_RULE_CONDITIONS: usize = 16;
/// 一个条件最多几个取值。
pub const MAX_CONDITION_VALUES: usize = 100;
/// 规则名称最多几个字符。
pub const MAX_RULE_NAME_CHARS: usize = 128;
/// 标签键、取值和正则最多几个字符。
pub const MAX_CONDITION_TEXT_CHARS: usize = 512;

/// 服务名依次取的标签。都没有时取事件的受影响服务里的第一个。
pub const SERVICE_LABELS: [&str; 2] = ["service.name", "service"];
/// 服务的命名空间标签；没有时是 APM 的默认命名空间。
pub const SERVICE_NAMESPACE_LABEL: &str = "service.namespace";
/// 写团队名的标签（不区分大小写）。
pub const TEAM_LABEL: &str = "team";
/// 写团队 id 的标签。
pub const TEAM_ID_LABEL: &str = "team_id";

/// 事件从哪来，路由规则可以按它筛。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentOrigin {
    /// 告警规则（含 APM、用户体验规则）。
    Rule,
    /// 外部告警来源。
    External,
    /// 拨测检查。
    Synthetic,
    /// 有人手动建的。
    Manual,
}

impl IncidentOrigin {
    /// 按事件的 `alert.source` 标签认出来源。
    pub fn of(labels: &std::collections::BTreeMap<String, String>) -> Self {
        match labels.get(SOURCE_LABEL).map(String::as_str) {
            Some(EXTERNAL_SOURCE) => Self::External,
            Some(SYNTHETIC_SOURCE) => Self::Synthetic,
            Some(MANUAL_SOURCE) => Self::Manual,
            _ => Self::Rule,
        }
    }
}

/// 条件怎么比。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOperator {
    /// 等于唯一的取值。
    Equals,
    /// 不等于唯一的取值；标签不存在也算不等于。
    NotEquals,
    /// 等于取值中的任何一个。
    In,
    /// 匹配唯一的正则（整个值都要匹配）。
    Matches,
}

/// 对事件一个标签的条件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelCondition {
    pub key: String,
    pub operator: ConditionOperator,
    pub values: Vec<String>,
}

/// 规则命中后交给谁。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RoutingTarget {
    /// 一条升级策略；团队仍按服务归属和标签定。
    Policy { policy_id: Id },
    /// 一个团队，走它的值班策略；团队没设值班策略时走默认策略。
    Team { team_id: Id },
}

/// 组织的一条路由规则。条件全部满足、严重度和来源也对得上才算命中。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingRule {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub enabled: bool,
    /// 在列表里的位置，小的先比。
    pub position: i32,
    pub conditions: Vec<LabelCondition>,
    /// 事件严重度不低于它才命中；空着不限。
    pub min_severity: Option<Severity>,
    /// 只对这些来源的事件生效；空着不限。
    pub origins: Vec<IncidentOrigin>,
    /// 只在这些时段（或不在这些时段）开的事件生效，例如白天交给开发团队、夜里交给值班团队；空着
    /// 不限。按事件开出来的时刻算。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<TimeCondition>,
    /// 只对这些优先级的事件生效；空着不限（没设优先级的事件也命中）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub priorities: Vec<Priority>,
    pub target: RoutingTarget,
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl RoutingRule {
    /// 名称、条件个数、取值个数与长度、运算符要的取值个数、正则能否编译。
    pub fn validate(&self) -> Result<()> {
        let name = self.name.trim();

        if name.is_empty() {
            return Err(Error::invalid("name cannot be empty"));
        }

        if name.chars().count() > MAX_RULE_NAME_CHARS {
            return Err(Error::invalid(format!(
                "name has at most {MAX_RULE_NAME_CHARS} characters"
            )));
        }

        if self.conditions.len() > MAX_RULE_CONDITIONS {
            return Err(Error::invalid(format!(
                "a routing rule has at most {MAX_RULE_CONDITIONS} conditions"
            )));
        }

        for condition in &self.conditions {
            condition.validate()?;
        }

        if let Some(time) = &self.time {
            time.validate()?;
        }

        Ok(())
    }

    /// 时间条件引用的节假日日历。
    pub fn calendar(&self) -> Option<&CalendarUse> {
        self.time
            .as_ref()
            .and_then(|time| time.windows.calendar.as_ref())
    }
}

/// 事件标签满足所有条件（工单的自动建单也按它判断）。正则按整串匹配，编不了的算不满足。
pub fn conditions_hold(
    conditions: &[LabelCondition],
    labels: &std::collections::BTreeMap<String, String>,
) -> bool {
    conditions.iter().all(|condition| {
        let pattern = match condition.operator {
            ConditionOperator::Matches => condition
                .values
                .first()
                .and_then(|value| decide::anchored(value).ok()),
            _ => None,
        };

        decide::holds(condition, pattern.as_ref(), labels)
    })
}

/// 一组标签条件说得通：个数、取值个数与长度、运算符要的取值个数、正则能否编译。
pub fn validate_label_conditions(conditions: &[LabelCondition]) -> Result<()> {
    if conditions.len() > MAX_RULE_CONDITIONS {
        return Err(Error::invalid(format!(
            "at most {MAX_RULE_CONDITIONS} conditions"
        )));
    }

    conditions.iter().try_for_each(LabelCondition::validate)
}

impl LabelCondition {
    fn validate(&self) -> Result<()> {
        let key = self.key.trim();

        if key.is_empty() || key.chars().count() > MAX_CONDITION_TEXT_CHARS {
            return Err(Error::invalid(format!(
                "a condition's label key has 1-{MAX_CONDITION_TEXT_CHARS} characters"
            )));
        }

        if self.values.len() > MAX_CONDITION_VALUES {
            return Err(Error::invalid(format!(
                "a condition has at most {MAX_CONDITION_VALUES} values"
            )));
        }

        if self
            .values
            .iter()
            .any(|value| value.chars().count() > MAX_CONDITION_TEXT_CHARS)
        {
            return Err(Error::invalid(format!(
                "a condition's value has at most {MAX_CONDITION_TEXT_CHARS} characters"
            )));
        }

        let single = matches!(
            self.operator,
            ConditionOperator::Equals | ConditionOperator::NotEquals | ConditionOperator::Matches
        );

        if single && self.values.len() != 1 {
            return Err(Error::invalid(format!(
                "the condition on `{key}` takes exactly one value"
            )));
        }

        if self.values.is_empty() {
            return Err(Error::invalid(format!(
                "the condition on `{key}` needs at least one value"
            )));
        }

        if self.operator == ConditionOperator::Matches {
            decide::anchored(&self.values[0]).map_err(|error| {
                Error::invalid(format!("the pattern for `{key}` is not valid: {error}"))
            })?;
        }

        Ok(())
    }
}

/// 一个团队的值班策略：交给这个团队的事件默认走它。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamPolicy {
    pub org_id: Id,
    pub team_id: Id,
    pub escalation_policy_id: Id,
    pub updated_by: Option<Id>,
    pub updated_at: TimestampMicros,
}

/// 事件是按什么分派的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingReason {
    /// 一条路由规则命中了。
    Rule,
    /// 服务登记了负责团队。
    ServiceOwner,
    /// 标签 `team` 或 `team_id` 指向一个团队。
    TeamLabel,
    /// 没有路由能定，走默认策略。
    Default,
    /// 来源明确选了升级策略或不升级；只按服务归属和标签定团队。
    Source,
    /// 手动建事件的人选了团队、升级策略或成员。
    Manual,
}

/// 事件上记下的分派经过。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentRouting {
    pub reason: RoutingReason,
    /// 命中的规则，规则改名或删除后仍按开事件那一刻的样子显示。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_name: Option<String>,
    /// 定团队用到的服务：`命名空间/服务名`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

#[async_trait]
pub trait RoutingRuleRepository: Send + Sync {
    async fn create(&self, rule: RoutingRule) -> Result<RoutingRule>;
    async fn update(&self, rule: RoutingRule) -> Result<RoutingRule>;
    /// 有就改、没有就建：联邦同步来的规则用它。
    async fn upsert(&self, rule: RoutingRule) -> Result<RoutingRule>;
    async fn get(&self, id: &Id) -> Result<RoutingRule>;
    /// 一个组织的规则，按位置排好。
    async fn list(&self, org_id: &Id) -> Result<Vec<RoutingRule>>;
    async fn delete(&self, id: &Id) -> Result<()>;
    /// 按给出的顺序重排一个组织的规则；没列出的排在后面，保持原来的先后。
    async fn reorder(&self, org_id: &Id, ids: &[Id]) -> Result<Vec<RoutingRule>>;
}

/// 服务归哪个团队：APM 服务目录登记了负责团队、没归档的服务。
#[async_trait]
pub trait ServiceOwnerSource: Send + Sync {
    async fn owners(&self, org_id: &Id) -> Result<Vec<ServiceOwner>>;
}

#[async_trait]
pub trait TeamPolicyRepository: Send + Sync {
    async fn list(&self, org_id: &Id) -> Result<Vec<TeamPolicy>>;
    async fn upsert(&self, policy: TeamPolicy) -> Result<TeamPolicy>;
    async fn delete(&self, org_id: &Id, team_id: &Id) -> Result<()>;
}
