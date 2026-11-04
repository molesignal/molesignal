// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 手动建事件：客户报障、有人发现的问题，规则没抓到，也照常通知和升级。
//!
//! - 带标签 `alert.source = manual`，`rule_id` 记为 [`MANUAL_RULE_ID`]（洞察里归成一项「手动创建」），
//!   注解 `created_by` 记创建人，说明记在注解 `description`；指纹每次新算，不去重。
//! - 不经过告警流水线：人写的内容不再改。只有「按路由分派」才走路由；交给团队或升级策略时直接用
//!   它的策略；指定成员时不用升级策略，只直接通知这些人。紧急度没指定时按事件的升级策略定。
//! - 没有规则会解决它，要人来解决。

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{Actor, IncidentActions, assign::MAX_ASSIGNEES};
use crate::{
    app::notify::{ALERT_TRIGGERED_EVENT, alert_dispatch},
    domain::alerting::{
        escalation::DEFAULT_POLICY_ID,
        handling::{EscalationReason, IncidentHandling},
        incident::{
            CREATED_BY_ANNOTATION, DESCRIPTION_ANNOTATION, Incident, IncidentStatus,
            MANUAL_RULE_ID, MANUAL_SOURCE, SOURCE_LABEL, Severity,
        },
        priority::Priority,
        routing::{IncidentRouting, RoutingReason},
        urgency::Urgency,
    },
    shared::{Error, Result, ids::Id},
};

/// 标题最长几个字符。
const MAX_TITLE_CHARS: usize = 200;

/// 说明最长几个字符。
const MAX_DESCRIPTION_CHARS: usize = 20_000;

/// 最多几个服务、几个标签。
const MAX_SERVICES: usize = 20;
const MAX_LABELS: usize = 50;

/// 交给谁。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum NewIncidentAssignment {
    /// 和规则开的事件一样按路由分派（路由规则、服务的负责团队、团队标签、默认策略）。
    #[default]
    Auto,
    /// 归到这个团队，用它的值班策略（没设就用默认策略）。
    Team { team_id: Id },
    /// 用这条升级策略，团队不定。
    Policy { escalation_policy_id: Id },
    /// 只直接通知这几个人，不用升级策略。
    Users { user_ids: Vec<Id> },
}

/// 手动建事件时给的内容。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewIncident {
    pub title: String,
    /// Markdown。
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_severity")]
    pub severity: Severity,
    /// 不给时按事件的升级策略定。
    #[serde(default)]
    pub urgency: Option<Urgency>,
    #[serde(default)]
    pub priority: Option<Priority>,
    #[serde(default)]
    pub assign: NewIncidentAssignment,
    /// 受影响的服务（服务目录里的名字）。
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// 同时宣布为重大事件；由接口层接着宣布。
    #[serde(default)]
    pub declare_major: bool,
}

fn default_severity() -> Severity {
    Severity::Error
}

impl NewIncident {
    fn validate(&self) -> Result<()> {
        let title = self.title.trim();

        if title.is_empty() {
            return Err(Error::invalid("an incident needs a title"));
        }

        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(Error::invalid(format!(
                "an incident title has at most {MAX_TITLE_CHARS} characters"
            )));
        }

        if self.description.chars().count() > MAX_DESCRIPTION_CHARS {
            return Err(Error::invalid(format!(
                "an incident description has at most {MAX_DESCRIPTION_CHARS} characters"
            )));
        }

        if self.services.len() > MAX_SERVICES {
            return Err(Error::invalid(format!(
                "an incident names at most {MAX_SERVICES} services"
            )));
        }

        if self.labels.len() > MAX_LABELS {
            return Err(Error::invalid(format!(
                "an incident has at most {MAX_LABELS} labels"
            )));
        }

        if self.labels.keys().any(|key| key.trim().is_empty()) {
            return Err(Error::invalid("a label needs a name"));
        }

        if let NewIncidentAssignment::Users { user_ids } = &self.assign
            && (user_ids.is_empty() || user_ids.len() > MAX_ASSIGNEES)
        {
            return Err(Error::invalid(format!(
                "name between 1 and {MAX_ASSIGNEES} people"
            )));
        }

        Ok(())
    }
}

/// 拼出还没分派、还没写库的事件。
fn manual_incident(actor: &Actor, new: &NewIncident) -> Incident {
    let id = Id::new();

    let mut labels: BTreeMap<String, String> = new
        .labels
        .iter()
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect();
    labels.insert(SOURCE_LABEL.into(), MANUAL_SOURCE.into());

    let mut annotations = BTreeMap::new();
    annotations.insert(CREATED_BY_ANNOTATION.to_string(), actor.user_id.0.clone());

    let description = new.description.trim();

    if !description.is_empty() {
        annotations.insert(DESCRIPTION_ANNOTATION.to_string(), description.to_string());
    }

    let services: Vec<String> = new
        .services
        .iter()
        .map(|service| service.trim().to_string())
        .filter(|service| !service.is_empty())
        .collect();

    Incident {
        fingerprint: manual_fingerprint(&id),
        id,
        org_id: actor.org_id.clone(),
        rule_id: Id::from_string(MANUAL_RULE_ID),
        escalation_policy_id: Id::from_string(DEFAULT_POLICY_ID),
        status: IncidentStatus::Open,
        severity: new.severity,
        summary: new.title.trim().to_string(),
        current_step: 0,
        current_loop: 0,
        current_step_started_at: actor.at,
        assignees: Vec::new(),
        labels,
        annotations,
        trace_ids: Vec::new(),
        host_ids: Vec::new(),
        affected_services: services,
        triggering_query: None,
        created_at: actor.at,
        acknowledged_at: None,
        acknowledged_by: None,
        resolved_at: None,
        resolved_by: None,
        team_id: None,
        routing: None,
        handling: IncidentHandling {
            priority: new.priority,
            urgency: new.urgency.clone(),
            ..IncidentHandling::default()
        },
        revision: 0,
    }
}

/// 每个手动建的事件一个指纹：不和任何事件去重。
fn manual_fingerprint(id: &Id) -> String {
    blake3::hash(format!("manual|{id}").as_bytes())
        .to_hex()
        .to_string()
}

/// 创建人指定的去向：不是路由定的。
fn chosen_by_creator() -> IncidentRouting {
    IncidentRouting {
        reason: RoutingReason::Manual,
        rule_id: None,
        rule_name: None,
        service: None,
    }
}

impl IncidentActions {
    /// 手动建事件：定去向和紧急度，写库，发「已触发」通知；指定成员时直接通知他们，其余的由调度器
    /// 从升级策略的第一级通知。
    pub async fn create(&self, actor: &Actor, new: NewIncident) -> Result<Incident> {
        new.validate()?;

        let levels = self.levels(&actor.org_id).await;

        if let Some(priority) = new.priority {
            levels.check_priority(priority)?;
        }

        if let Some(urgency) = &new.urgency {
            levels.check_urgency(urgency)?;
        }

        let mut incident = manual_incident(actor, &new);
        let mut paged = Vec::new();

        incident = match &new.assign {
            NewIncidentAssignment::Auto => self.routing.router().route(incident).await,
            NewIncidentAssignment::Team { team_id } => {
                incident.escalation_policy_id =
                    self.routing.team_on_call(&actor.org_id, team_id).await?;
                incident.team_id = Some(team_id.clone());
                incident.routing = Some(chosen_by_creator());
                self.routing.router().settle_urgency(incident).await
            }
            NewIncidentAssignment::Policy {
                escalation_policy_id,
            } => {
                self.routing
                    .check_policy(&actor.org_id, escalation_policy_id)
                    .await?;

                incident.escalation_policy_id = escalation_policy_id.clone();
                incident.routing = Some(chosen_by_creator());
                self.routing.router().settle_urgency(incident).await
            }
            NewIncidentAssignment::Users { user_ids } => {
                paged = self.members(actor, user_ids).await?;

                // 没有升级策略：调度器不碰它，只通知这几个人。
                incident.escalation_policy_id = Id::from_string("");
                incident.assignees = paged.clone();
                incident.handling.assignees_pinned = true;
                incident.routing = Some(chosen_by_creator());
                self.routing.router().settle_urgency(incident).await
            }
        };

        let incident = self.incidents.create(incident).await?;

        self.emit(&incident).await;

        if let Some(engine) = &self.notify_engine
            && let Err(error) = engine
                .enqueue_event(alert_dispatch(&incident, ALERT_TRIGGERED_EVENT))
                .await
        {
            tracing::warn!(incident_id = %incident.id, %error, "alert notify event enqueue failed");
        }

        if !paged.is_empty() {
            let key = format!("{}:created", incident.id);
            self.page(&incident, &paged, EscalationReason::Manual, &key)
                .await;
        }

        Ok(incident)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::alerting::timeline::ActionVia, shared::time::TimestampMicros};

    fn actor() -> Actor {
        Actor {
            org_id: Id::from_string("org-1"),
            user_id: Id::from_string("u-1"),
            via: ActionVia::Web,
            at: TimestampMicros(1_000),
        }
    }

    fn new(json: serde_json::Value) -> NewIncident {
        serde_json::from_value(json).expect("new incident")
    }

    #[test]
    fn a_hand_made_incident_says_who_opened_it_and_never_dedups() {
        let request = new(serde_json::json!({
            "title": "  Payment page down in cn-north  ",
            "description": "Customers report a blank page.",
            "labels": {"env": "prod", "alert.source": "rule"},
            "services": ["checkout", " "],
        }));

        let first = manual_incident(&actor(), &request);
        let second = manual_incident(&actor(), &request);

        assert_eq!(first.summary, "Payment page down in cn-north");
        assert_eq!(first.severity, Severity::Error);
        assert_eq!(first.rule_id.as_str(), MANUAL_RULE_ID);
        assert_eq!(
            first.labels.get(SOURCE_LABEL).map(String::as_str),
            Some(MANUAL_SOURCE)
        );
        assert_eq!(first.labels.get("env").map(String::as_str), Some("prod"));
        assert_eq!(
            first
                .annotations
                .get(CREATED_BY_ANNOTATION)
                .map(String::as_str),
            Some("u-1")
        );
        assert_eq!(
            first
                .annotations
                .get(DESCRIPTION_ANNOTATION)
                .map(String::as_str),
            Some("Customers report a blank page.")
        );
        assert_eq!(first.affected_services, vec!["checkout".to_string()]);
        assert_ne!(first.fingerprint, second.fingerprint);
        assert_eq!(first.handling.urgency, None);
    }

    #[test]
    fn a_request_without_a_title_or_with_too_many_people_is_refused() {
        assert!(new(serde_json::json!({"title": "  "})).validate().is_err());

        let crowd: Vec<String> = (0..=MAX_ASSIGNEES).map(|n| format!("u-{n}")).collect();
        let too_many = new(serde_json::json!({
            "title": "Down",
            "assign": {"kind": "users", "userIds": crowd},
        }));
        assert!(too_many.validate().is_err());

        let nobody =
            new(serde_json::json!({"title": "Down", "assign": {"kind": "users", "userIds": []}}));
        assert!(nobody.validate().is_err());

        let chosen = new(serde_json::json!({
            "title": "Down",
            "severity": "critical",
            "urgency": "low",
            "priority": "P2",
            "assign": {"kind": "team", "teamId": "team-1"},
        }));
        assert!(chosen.validate().is_ok());
        assert_eq!(
            chosen.assign,
            NewIncidentAssignment::Team {
                team_id: Id::from_string("team-1")
            }
        );
    }
}
