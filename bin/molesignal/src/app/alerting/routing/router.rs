// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 路由器：事件开出来、写库之前，按组织的路由表定下团队和升级策略。
//!
//! 路由表由路由规则、团队的值班策略、团队、升级策略和服务归属拼成，按组织缓存 30 秒；
//! 本节点改了路由配置就当场作废。读不到路由表时事件照来源给的样子开，不因为路由耽误告警。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{
    app::alerting::importance::{ImportanceService, levels_of},
    domain::{
        alerting::{
            escalation::EscalationPolicy,
            importance::ImportanceLevels,
            incident::Incident,
            repositories::EscalationPolicyRepository,
            routing::{
                RoutingRuleRepository, RoutingSubject, RoutingTable, ServiceOwnerSource,
                TeamPolicyRepository,
            },
        },
        iam::TeamRepository,
    },
    shared::{Result, ids::Id},
};

/// 一个组织的路由表，和定紧急度要用的升级策略。
#[derive(Clone)]
struct Loaded {
    table: Arc<RoutingTable>,
    policies: Arc<Vec<EscalationPolicy>>,
}

/// 路由表在缓存里留多久。别的节点改的配置最多晚这么久生效。
const TABLE_TTL: Duration = Duration::from_secs(30);

pub struct IncidentRouter {
    rules: Arc<dyn RoutingRuleRepository>,
    team_policies: Arc<dyn TeamPolicyRepository>,
    teams: Arc<dyn TeamRepository>,
    policies: Arc<dyn EscalationPolicyRepository>,
    owners: Option<Arc<dyn ServiceOwnerSource>>,
    importance: Option<Arc<ImportanceService>>,
    tables: Mutex<HashMap<Id, (Instant, Loaded)>>,
}

impl IncidentRouter {
    pub fn new(
        rules: Arc<dyn RoutingRuleRepository>,
        team_policies: Arc<dyn TeamPolicyRepository>,
        teams: Arc<dyn TeamRepository>,
        policies: Arc<dyn EscalationPolicyRepository>,
        owners: Option<Arc<dyn ServiceOwnerSource>>,
    ) -> Self {
        Self {
            rules,
            team_policies,
            teams,
            policies,
            owners,
            importance: None,
            tables: Mutex::new(HashMap::new()),
        }
    }

    /// 紧急度按组织的档位定，优先级落进组织的档数。
    pub fn with_importance(mut self, importance: Arc<ImportanceService>) -> Self {
        self.importance = Some(importance);
        self
    }

    /// 本节点改了组织的路由配置：下一个事件重新读。
    pub fn forget(&self, org_id: &Id) {
        if let Ok(mut tables) = self.tables.lock() {
            tables.remove(org_id);
        }
    }

    pub async fn table(&self, org_id: &Id) -> Result<Arc<RoutingTable>> {
        Ok(self.loaded(org_id).await?.table)
    }

    async fn loaded(&self, org_id: &Id) -> Result<Loaded> {
        let cached = self
            .tables
            .lock()
            .ok()
            .and_then(|tables| tables.get(org_id).cloned())
            .filter(|(loaded, _)| loaded.elapsed() < TABLE_TTL)
            .map(|(_, loaded)| loaded);

        if let Some(loaded) = cached {
            return Ok(loaded);
        }

        let loaded = self.load(org_id).await?;

        if let Ok(mut tables) = self.tables.lock() {
            tables.insert(org_id.clone(), (Instant::now(), loaded.clone()));
        }

        Ok(loaded)
    }

    /// 给刚拼好、还没写库的事件定团队和升级策略，再按策略定紧急度（事件明确指定的不动）。
    pub async fn route(&self, mut incident: Incident) -> Incident {
        let levels = levels_of(self.importance.as_ref(), &incident.org_id).await;
        fit_priority(&mut incident, &levels);

        let loaded = match self.loaded(&incident.org_id).await {
            Ok(loaded) => loaded,
            Err(error) => {
                tracing::warn!(
                    org_id = %incident.org_id,
                    error = %error,
                    "alert routing table unreadable; the incident keeps its source's policy"
                );
                incident
                    .handling
                    .urgency
                    .get_or_insert_with(|| levels.by_severity(incident.severity));
                return incident;
            }
        };

        let decision = loaded.table.decide(&RoutingSubject {
            labels: &incident.labels,
            affected_services: &incident.affected_services,
            severity: incident.severity,
            priority: incident.handling.priority,
            at: incident.created_at,
            source_policy: &incident.escalation_policy_id,
        });

        incident.escalation_policy_id = decision.escalation_policy_id;
        incident.team_id = decision.team_id;
        incident.routing = Some(decision.routing);

        urgency_by_policy(&mut incident, &loaded.policies, &levels);
        incident
    }

    /// 去向已经定了的事件（手动建事件时选了团队、策略或成员）：只按它的升级策略定紧急度，事件
    /// 明确指定的不动；没有策略时按严重度。
    pub async fn settle_urgency(&self, mut incident: Incident) -> Incident {
        let levels = levels_of(self.importance.as_ref(), &incident.org_id).await;
        fit_priority(&mut incident, &levels);

        match self.loaded(&incident.org_id).await {
            Ok(loaded) => urgency_by_policy(&mut incident, &loaded.policies, &levels),
            Err(error) => {
                tracing::warn!(
                    org_id = %incident.org_id,
                    error = %error,
                    "escalation policies unreadable; urgency follows the severity"
                );
                incident
                    .handling
                    .urgency
                    .get_or_insert_with(|| levels.by_severity(incident.severity));
            }
        }

        incident
    }

    async fn load(&self, org_id: &Id) -> Result<Loaded> {
        let rules = self.rules.list(org_id).await?;
        let team_policies = self.team_policies.list(org_id).await?;
        let teams = self.teams.list(org_id).await?;
        let policies = self.policies.list(org_id).await?;

        let owners = match &self.owners {
            Some(owners) => owners.owners(org_id).await?,
            None => Vec::new(),
        };

        let table = RoutingTable::new(
            rules,
            team_policies,
            owners,
            teams.into_iter().map(|team| (team.id, team.name)).collect(),
            policies.iter().map(|policy| policy.id.clone()).collect(),
        );

        Ok(Loaded {
            table: Arc::new(table),
            policies: Arc::new(policies),
        })
    }
}

/// 事件没明确指定紧急度时，按它的升级策略的紧急度规则定，并记下是按策略定的。
fn urgency_by_policy(
    incident: &mut Incident,
    policies: &[EscalationPolicy],
    levels: &ImportanceLevels,
) {
    if incident.handling.urgency.is_some() {
        return;
    }

    let rule = EscalationPolicy::resolve(policies, &incident.escalation_policy_id)
        .map(|policy| policy.urgency_rule.clone())
        .unwrap_or_default();

    incident.handling.urgency = Some(rule.urgency(incident.severity, incident.created_at, levels));
    incident.handling.urgency_by_policy = true;
}

/// 外部告警、流水线给的优先级超出组织的档数时，落到最低一档；给的紧急度不是组织的一档时不算，交给
/// 升级策略定。
fn fit_priority(incident: &mut Incident, levels: &ImportanceLevels) {
    incident.handling.priority = incident
        .handling
        .priority
        .map(|priority| levels.fit_priority(priority));

    if incident
        .handling
        .urgency
        .as_ref()
        .is_some_and(|urgency| !levels.has_urgency(urgency))
    {
        incident.handling.urgency = None;
    }
}
