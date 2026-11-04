// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 路由的配置：路由规则、团队的值班策略，以及「试一试」。分派本身在 [`IncidentRouter`]。
//!
//! 路由配置不在联邦的集群之间同步：团队和 APM 服务目录都是各集群自己的，路由跟着它们；
//! 升级策略是同步的，指向升级策略的规则在每个集群都能用。

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        alerting::{
            incident::Severity,
            priority::Priority,
            repositories::EscalationPolicyRepository,
            routing::{
                IncidentOrigin, LabelCondition, MAX_ROUTING_RULES, RoutingExplanation, RoutingRule,
                RoutingRuleRepository, RoutingSubject, RoutingTarget, ServiceOwner,
                ServiceOwnerSource, TeamPolicy, TeamPolicyRepository,
            },
            time_window::TimeCondition,
        },
        apm::ApmCatalogRepository,
        iam::TeamRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod router;

pub use router::IncidentRouter;

/// 新建或修改一条规则时给的内容；位置另由排序决定。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleWrite {
    pub name: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub conditions: Vec<LabelCondition>,
    #[serde(default)]
    pub min_severity: Option<Severity>,
    #[serde(default)]
    pub origins: Vec<IncidentOrigin>,
    #[serde(default)]
    pub time: Option<TimeCondition>,
    #[serde(default)]
    pub priorities: Vec<Priority>,
    pub target: RoutingTarget,
}

fn enabled_by_default() -> bool {
    true
}

/// 「试一试」的输入：一组标签、受影响的服务和严重度，按默认策略分派。
#[derive(Debug, Clone, Deserialize)]
pub struct RoutingProbe {
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default = "probe_severity")]
    pub severity: Severity,
    #[serde(default)]
    pub priority: Option<Priority>,
    /// 按这一刻试（微秒）；不给是现在。
    #[serde(default)]
    pub at: Option<i64>,
}

fn probe_severity() -> Severity {
    Severity::Warning
}

/// APM 服务目录里的归属：登记了负责团队、没归档的服务。
pub struct CatalogOwners(pub Arc<dyn ApmCatalogRepository>);

#[async_trait]
impl ServiceOwnerSource for CatalogOwners {
    async fn owners(&self, org_id: &Id) -> Result<Vec<ServiceOwner>> {
        Ok(self
            .0
            .list_definitions(org_id)
            .await?
            .into_iter()
            .filter(|definition| !definition.archived)
            .filter_map(|definition| {
                Some(ServiceOwner {
                    team_id: definition.team_id?,
                    namespace: definition.namespace,
                    name: definition.name,
                })
            })
            .collect())
    }
}

/// 路由页上的一个团队：它的值班策略和它负责的服务。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamRouting {
    pub team_id: Id,
    pub name: String,
    pub member_count: usize,
    pub escalation_policy_id: Option<Id>,
    /// 服务目录里登记给它的服务：`命名空间/服务名`。
    pub services: Vec<String>,
}

pub struct AlertRouting {
    rules: Arc<dyn RoutingRuleRepository>,
    team_policies: Arc<dyn TeamPolicyRepository>,
    teams: Arc<dyn TeamRepository>,
    policies: Arc<dyn EscalationPolicyRepository>,
    owners: Option<Arc<dyn ServiceOwnerSource>>,
    router: Arc<IncidentRouter>,
}

impl AlertRouting {
    pub fn new(
        rules: Arc<dyn RoutingRuleRepository>,
        team_policies: Arc<dyn TeamPolicyRepository>,
        teams: Arc<dyn TeamRepository>,
        policies: Arc<dyn EscalationPolicyRepository>,
        owners: Option<Arc<dyn ServiceOwnerSource>>,
    ) -> Self {
        let router = Arc::new(IncidentRouter::new(
            rules.clone(),
            team_policies.clone(),
            teams.clone(),
            policies.clone(),
            owners.clone(),
        ));

        Self {
            rules,
            team_policies,
            teams,
            policies,
            owners,
            router,
        }
    }

    /// 路由器按组织的档位定紧急度、落优先级。
    pub fn with_importance(
        mut self,
        importance: Arc<crate::app::alerting::ImportanceService>,
    ) -> Self {
        self.router = Arc::new(
            IncidentRouter::new(
                self.rules.clone(),
                self.team_policies.clone(),
                self.teams.clone(),
                self.policies.clone(),
                self.owners.clone(),
            )
            .with_importance(importance),
        );
        self
    }

    /// 开事件的各条路径共用的路由器。
    pub fn router(&self) -> Arc<IncidentRouter> {
        self.router.clone()
    }

    pub async fn rules(&self, org_id: &Id) -> Result<Vec<RoutingRule>> {
        self.rules.list(org_id).await
    }

    pub async fn create_rule(
        &self,
        org_id: &Id,
        user_id: &Id,
        write: RuleWrite,
    ) -> Result<RoutingRule> {
        let existing = self.rules.list(org_id).await?;

        if existing.len() >= MAX_ROUTING_RULES {
            return Err(Error::invalid(format!(
                "an organization has at most {MAX_ROUTING_RULES} routing rules"
            )));
        }

        let now = TimestampMicros::now();
        let rule = RoutingRule {
            id: Id::new(),
            org_id: org_id.clone(),
            name: write.name.trim().to_string(),
            enabled: write.enabled,
            position: existing
                .iter()
                .map(|rule| rule.position + 1)
                .max()
                .unwrap_or(0),
            conditions: write.conditions,
            min_severity: write.min_severity,
            origins: write.origins,
            time: write.time,
            priorities: write.priorities,
            target: write.target,
            created_by: Some(user_id.clone()),
            created_at: now,
            updated_at: now,
        };

        self.check(&rule).await?;

        let created = self.rules.create(rule).await?;
        self.router.forget(org_id);
        Ok(created)
    }

    /// 照保存时那样检查一条要写的规则，不写库：导入预演用。
    pub async fn check_write(&self, org_id: &Id, write: &RuleWrite) -> Result<()> {
        let now = TimestampMicros::now();
        let rule = RoutingRule {
            id: Id::new(),
            org_id: org_id.clone(),
            name: write.name.trim().to_string(),
            enabled: write.enabled,
            position: 0,
            conditions: write.conditions.clone(),
            min_severity: write.min_severity,
            origins: write.origins.clone(),
            time: write.time.clone(),
            priorities: write.priorities.clone(),
            target: write.target.clone(),
            created_by: None,
            created_at: now,
            updated_at: now,
        };

        self.check(&rule).await
    }

    pub async fn update_rule(&self, org_id: &Id, id: &Id, write: RuleWrite) -> Result<RoutingRule> {
        let current = self.rule(org_id, id).await?;
        let rule = RoutingRule {
            name: write.name.trim().to_string(),
            enabled: write.enabled,
            conditions: write.conditions,
            min_severity: write.min_severity,
            origins: write.origins,
            time: write.time,
            priorities: write.priorities,
            target: write.target,
            updated_at: TimestampMicros::now(),
            ..current
        };

        self.check(&rule).await?;

        let updated = self.rules.update(rule).await?;
        self.router.forget(org_id);
        Ok(updated)
    }

    pub async fn delete_rule(&self, org_id: &Id, id: &Id) -> Result<RoutingRule> {
        let rule = self.rule(org_id, id).await?;

        self.rules.delete(id).await?;
        self.router.forget(org_id);
        Ok(rule)
    }

    pub async fn reorder(&self, org_id: &Id, ids: &[Id]) -> Result<Vec<RoutingRule>> {
        let rules = self.rules.reorder(org_id, ids).await?;
        self.router.forget(org_id);
        Ok(rules)
    }

    /// 组织里的团队，各自的值班策略和负责的服务，按名字排。
    pub async fn teams(&self, org_id: &Id) -> Result<Vec<TeamRouting>> {
        let teams = self.teams.list(org_id).await?;
        let policies: BTreeMap<String, Id> = self
            .team_policies
            .list(org_id)
            .await?
            .into_iter()
            .map(|policy| (policy.team_id.0, policy.escalation_policy_id))
            .collect();

        let mut services: BTreeMap<String, Vec<String>> = BTreeMap::new();

        if let Some(owners) = &self.owners {
            for owner in owners.owners(org_id).await? {
                services
                    .entry(owner.team_id.0)
                    .or_default()
                    .push(format!("{}/{}", owner.namespace, owner.name));
            }
        }

        let mut rows: Vec<TeamRouting> = teams
            .into_iter()
            .map(|team| TeamRouting {
                escalation_policy_id: policies.get(&team.id.0).cloned(),
                services: services.remove(&team.id.0).unwrap_or_default(),
                member_count: team.member_ids.len(),
                name: team.name,
                team_id: team.id,
            })
            .collect();

        rows.sort_by_key(|row| row.name.to_lowercase());
        Ok(rows)
    }

    /// 设置或去掉一个团队的值班策略（`None` 去掉）。
    pub async fn set_team_policy(
        &self,
        org_id: &Id,
        user_id: &Id,
        team_id: &Id,
        policy_id: Option<Id>,
    ) -> Result<Option<TeamPolicy>> {
        self.team(org_id, team_id).await?;

        let result = match policy_id {
            Some(policy_id) => {
                self.policy(org_id, &policy_id).await?;

                Some(
                    self.team_policies
                        .upsert(TeamPolicy {
                            org_id: org_id.clone(),
                            team_id: team_id.clone(),
                            escalation_policy_id: policy_id,
                            updated_by: Some(user_id.clone()),
                            updated_at: TimestampMicros::now(),
                        })
                        .await?,
                )
            }
            None => {
                self.team_policies.delete(org_id, team_id).await?;
                None
            }
        };

        self.router.forget(org_id);
        Ok(result)
    }

    /// 转派给团队时用：团队在本组织里，返回它的值班策略；没设或那条策略已删时是默认策略的别名。
    pub async fn team_on_call(&self, org_id: &Id, team_id: &Id) -> Result<Id> {
        self.team(org_id, team_id).await?;

        let assigned = self
            .team_policies
            .list(org_id)
            .await?
            .into_iter()
            .find(|policy| policy.team_id == *team_id)
            .map(|policy| policy.escalation_policy_id);

        if let Some(policy_id) = assigned
            && self.policy(org_id, &policy_id).await.is_ok()
        {
            return Ok(policy_id);
        }

        Ok(Id::from_string(
            crate::domain::alerting::escalation::DEFAULT_POLICY_ID,
        ))
    }

    /// 转派给升级策略时用：策略在本组织里。
    pub async fn check_policy(&self, org_id: &Id, policy_id: &Id) -> Result<()> {
        self.policy(org_id, policy_id).await
    }

    /// 按现在的路由表，这组标签会怎么分派（来源用默认策略时）。
    pub async fn explain(&self, org_id: &Id, probe: RoutingProbe) -> Result<RoutingExplanation> {
        let table = self.router.table(org_id).await?;
        let default_policy =
            Id::from_string(crate::domain::alerting::escalation::DEFAULT_POLICY_ID);

        Ok(table.explain(&RoutingSubject {
            labels: &probe.labels,
            affected_services: &probe.services,
            severity: probe.severity,
            priority: probe.priority,
            at: probe
                .at
                .map(TimestampMicros)
                .unwrap_or_else(TimestampMicros::now),
            source_policy: &default_policy,
        }))
    }

    async fn rule(&self, org_id: &Id, id: &Id) -> Result<RoutingRule> {
        let rule = self.rules.get(id).await?;

        if rule.org_id != *org_id {
            return Err(Error::not_found("routing rule"));
        }

        Ok(rule)
    }

    async fn team(&self, org_id: &Id, team_id: &Id) -> Result<()> {
        let team = self
            .teams
            .get(team_id)
            .await
            .map_err(|_| Error::invalid(format!("team `{}` does not exist", team_id.0)))?;

        if team.org_id != *org_id {
            return Err(Error::invalid(format!(
                "team `{}` does not exist",
                team_id.0
            )));
        }

        Ok(())
    }

    async fn policy(&self, org_id: &Id, policy_id: &Id) -> Result<()> {
        let exists = self
            .policies
            .list(org_id)
            .await?
            .iter()
            .any(|policy| policy.id == *policy_id);

        if !exists {
            return Err(Error::invalid(format!(
                "escalation policy `{}` does not exist in this organization",
                policy_id.0
            )));
        }

        Ok(())
    }

    async fn check(&self, rule: &RoutingRule) -> Result<()> {
        rule.validate()?;

        if let Some(calendar) = rule.calendar() {
            calendar.check(&rule.org_id)?;
        }

        match &rule.target {
            RoutingTarget::Policy { policy_id } => self.policy(&rule.org_id, policy_id).await,
            RoutingTarget::Team { team_id } => self.team(&rule.org_id, team_id).await,
        }
    }
}
