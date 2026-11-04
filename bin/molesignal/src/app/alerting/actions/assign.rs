// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 转派与增派。

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Actor, IncidentActions, check_actionable, reopen};
use crate::{
    domain::alerting::{
        escalation::EscalationPolicy, handling::EscalationReason, incident::Incident,
        timeline::TimelineKind,
    },
    shared::{Error, Result, ids::Id},
};

/// 一次最多指定几个人。
pub const MAX_ASSIGNEES: usize = 10;

/// 转派给谁。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Reassignment {
    /// 负责人换成这几个人，并通知他们；没人确认时照升级策略往下升级。
    Users { user_ids: Vec<Id> },
    /// 归到这个团队，改用它的值班策略（没设就用默认策略），从第一步重新开始。
    Team { team_id: Id },
    /// 改用这条升级策略，从第一步重新开始；团队不变。
    Policy { escalation_policy_id: Id },
}

impl IncidentActions {
    /// 转派。
    pub async fn reassign(
        &self,
        actor: &Actor,
        incident_id: &Id,
        to: Reassignment,
    ) -> Result<Incident> {
        match to {
            Reassignment::Users { user_ids } => {
                let users = self.members(actor, &user_ids).await?;
                self.reassign_to_users(actor, incident_id, users).await
            }
            Reassignment::Team { team_id } => {
                let policy_id = self.routing.team_on_call(&actor.org_id, &team_id).await?;
                self.restart(actor, incident_id, Some(team_id), policy_id)
                    .await
            }
            Reassignment::Policy {
                escalation_policy_id,
            } => {
                self.routing
                    .check_policy(&actor.org_id, &escalation_policy_id)
                    .await?;

                self.restart(actor, incident_id, None, escalation_policy_id)
                    .await
            }
        }
    }

    /// 增派：把这几个人加进负责人并通知他们，事件状态不变。都已经是负责人时什么也不做。
    pub async fn add_responders(
        &self,
        actor: &Actor,
        incident_id: &Id,
        user_ids: &[Id],
    ) -> Result<Incident> {
        let users = self.members(actor, user_ids).await?;
        let mut added = Vec::new();
        let mut lifted = false;

        let incident = self
            .modify(actor, incident_id, |incident| {
                check_actionable(incident)?;

                added = users
                    .iter()
                    .filter(|user| !incident.assignees.contains(user))
                    .cloned()
                    .collect();

                if added.is_empty() {
                    return Ok(false);
                }

                incident.assignees.extend(added.iter().cloned());
                incident.handling.assignees_pinned = true;
                lifted = incident.handling.lift_inhibition(&actor.user_id);
                Ok(true)
            })
            .await?;

        if added.is_empty() {
            return Ok(incident);
        }

        self.record_lift(actor, &incident, lifted).await;

        let key = self
            .record(
                actor,
                &incident,
                TimelineKind::RespondersAdded,
                json!({ "userIds": added }),
            )
            .await;

        self.page(&incident, &added, EscalationReason::ResponderAdded, &key)
            .await;

        Ok(incident)
    }

    async fn reassign_to_users(
        &self,
        actor: &Actor,
        incident_id: &Id,
        users: Vec<Id>,
    ) -> Result<Incident> {
        let mut before = Vec::new();
        let mut lifted = false;

        let incident = self
            .modify(actor, incident_id, |incident| {
                check_actionable(incident)?;

                before = incident.assignees.clone();
                incident.assignees = users.clone();
                incident.handling.assignees_pinned = true;
                lifted = incident.handling.lift_inhibition(&actor.user_id);
                reopen(incident, actor.at);
                Ok(true)
            })
            .await?;

        self.record_lift(actor, &incident, lifted).await;

        let detail = json!({
            "to": { "kind": "users", "userIds": users },
            "from": { "assignees": before },
        });

        let key = self
            .record(actor, &incident, TimelineKind::Reassigned, detail)
            .await;

        self.page(&incident, &users, EscalationReason::Reassigned, &key)
            .await;
        self.emit(&incident).await;

        Ok(incident)
    }

    /// 转给团队或升级策略：换成这条策略，从对这个严重度适用的第一步重新开始，立即通知那一步的对象。
    async fn restart(
        &self,
        actor: &Actor,
        incident_id: &Id,
        team_id: Option<Id>,
        policy_id: Id,
    ) -> Result<Incident> {
        let policies = self.policies.list(&actor.org_id).await?;

        let Some(policy) = EscalationPolicy::resolve(&policies, &policy_id).cloned() else {
            return Err(Error::invalid(
                "there is no escalation policy to restart with: the team has no on-call policy \
                 and the organization has no policy named `default`",
            ));
        };

        let mut from = json!({});
        let mut lifted = false;

        let incident = self
            .modify(actor, incident_id, |incident| {
                check_actionable(incident)?;

                let Some(first) = (0..policy.steps.len())
                    .find(|&index| policy.steps[index].applies_to(incident.severity))
                else {
                    return Err(Error::invalid(
                        "no step of that escalation policy applies to this incident's severity",
                    ));
                };

                from = json!({
                    "teamId": incident.team_id,
                    "escalationPolicyId": incident.escalation_policy_id,
                    "assignees": incident.assignees,
                });

                if let Some(team_id) = &team_id {
                    incident.team_id = Some(team_id.clone());
                }

                incident.escalation_policy_id = policy.id.clone();
                incident.current_step = first as u32;
                incident.current_loop = 0;
                incident.assignees.clear();
                incident.handling.assignees_pinned = false;
                incident.handling.next_round(EscalationReason::Reassigned);
                lifted = incident.handling.lift_inhibition(&actor.user_id);
                reopen(incident, actor.at);
                Ok(true)
            })
            .await?;

        self.record_lift(actor, &incident, lifted).await;

        let to = match &team_id {
            Some(team_id) => {
                json!({ "kind": "team", "teamId": team_id, "escalationPolicyId": policy.id })
            }
            None => json!({ "kind": "policy", "escalationPolicyId": policy.id }),
        };

        self.record(
            actor,
            &incident,
            TimelineKind::Reassigned,
            json!({ "to": to, "from": from }),
        )
        .await;

        self.page_step(&incident, &policy).await;
        self.emit(&incident).await;

        Ok(incident)
    }

    /// 这几个人都在本组织里，去掉重复，至少一个、至多 [`MAX_ASSIGNEES`] 个。
    pub(super) async fn members(&self, actor: &Actor, user_ids: &[Id]) -> Result<Vec<Id>> {
        let mut users: Vec<Id> = Vec::new();

        for user_id in user_ids {
            if !users.contains(user_id) {
                users.push(user_id.clone());
            }
        }

        if users.is_empty() {
            return Err(Error::invalid("name at least one person"));
        }

        if users.len() > MAX_ASSIGNEES {
            return Err(Error::invalid(format!(
                "at most {MAX_ASSIGNEES} people at a time"
            )));
        }

        let members = self.memberships.list_for_org(&actor.org_id).await?;

        if let Some(stranger) = users
            .iter()
            .find(|user| !members.iter().any(|member| member.user_id == **user))
        {
            return Err(Error::invalid(format!(
                "user `{stranger}` is not a member of this organization"
            )));
        }

        Ok(users)
    }
}
