// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 人对事件的处理：转派、增派、立即升级、暂缓、改严重度、合并、备注与批量处理，以及事件的时间线。
//! 确认、解决在 [`AlertingService`]。
//!
//! 每个操作读出最新的事件改好再写回；写回时比对 `revision`，期间被调度器、评估器或别人改过就重读
//! 再改。改好之后记一条时间线，需要通知的人立即通知，不等调度器的下一轮。权限在接口层按事件检查。

use std::sync::Arc;

use serde_json::{Value, json};

use super::{AlertIncidentLifecycleSink, AlertRouting, AlertingService, dispatcher};
use crate::{
    app::{
        alerting::service::WRITE_ATTEMPTS,
        notify::{NotifyEngine, alert_page_dispatch},
    },
    domain::{
        alerting::{
            escalation::EscalationPolicy,
            handling::EscalationReason,
            incident::{Incident, IncidentStatus},
            repositories::{EscalationPolicyRepository, IncidentRepository},
            timeline::{
                ActionVia, IncidentTimelineRepository, TimelineEntry, TimelineKind, note_text,
            },
        },
        iam::IamMembershipRepository,
        notify::repositories::NotifyDeliveryRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod assign;
mod bulk;
mod create;
mod escalate;
mod importance;
mod inhibition;
mod merge;
mod snooze;
mod timeline;

pub use assign::{MAX_ASSIGNEES, Reassignment};
pub use bulk::{BulkAction, BulkResult, MAX_BULK_INCIDENTS};
pub use create::{NewIncident, NewIncidentAssignment};
pub use merge::{IncidentMerger, MergedRecoverySink};
pub use timeline::{IncidentTimeline, TimelineDelivery};

/// 谁、从哪里、在什么时候做的。
#[derive(Debug, Clone)]
pub struct Actor {
    pub org_id: Id,
    pub user_id: Id,
    pub via: ActionVia,
    pub at: TimestampMicros,
}

impl Actor {
    pub fn new(org_id: &Id, user_id: &Id, via: ActionVia) -> Self {
        Self {
            org_id: org_id.clone(),
            user_id: user_id.clone(),
            via,
            at: TimestampMicros::now(),
        }
    }

    /// 系统自己做的（组合规则把子规则的事件并进来）：时间线上不记人。
    pub fn system(org_id: &Id) -> Self {
        Self::new(org_id, &Id::from_string(""), ActionVia::Api)
    }

    /// 时间线上记的人；系统做的没有。
    fn person(&self) -> Option<Id> {
        (!self.user_id.as_str().is_empty()).then(|| self.user_id.clone())
    }
}

pub struct IncidentActions {
    incidents: Arc<dyn IncidentRepository>,
    timeline: Arc<dyn IncidentTimelineRepository>,
    policies: Arc<dyn EscalationPolicyRepository>,
    memberships: Arc<dyn IamMembershipRepository>,
    routing: Arc<AlertRouting>,
    service: Arc<AlertingService>,
    notify_engine: Option<Arc<NotifyEngine>>,
    deliveries: Option<Arc<dyn NotifyDeliveryRepository>>,
    lifecycle_sink: Option<Arc<dyn AlertIncidentLifecycleSink>>,
    importance: Option<Arc<crate::app::alerting::ImportanceService>>,
}

impl IncidentActions {
    pub fn new(
        incidents: Arc<dyn IncidentRepository>,
        timeline: Arc<dyn IncidentTimelineRepository>,
        policies: Arc<dyn EscalationPolicyRepository>,
        memberships: Arc<dyn IamMembershipRepository>,
        routing: Arc<AlertRouting>,
        service: Arc<AlertingService>,
    ) -> Self {
        Self {
            incidents,
            timeline,
            policies,
            memberships,
            routing,
            service,
            notify_engine: None,
            deliveries: None,
            lifecycle_sink: None,
            importance: None,
        }
    }

    /// 改优先级、紧急度和手动建事件时按组织的档位检查。
    pub fn with_importance(
        mut self,
        importance: Arc<crate::app::alerting::ImportanceService>,
    ) -> Self {
        self.importance = Some(importance);
        self
    }

    /// 这个组织的档位；没接上档位服务时是内置的。
    pub(super) async fn levels(
        &self,
        org_id: &Id,
    ) -> Arc<crate::domain::alerting::importance::ImportanceLevels> {
        crate::app::alerting::importance::levels_of(self.importance.as_ref(), org_id).await
    }

    /// 接上通知：新的负责人立即收到通知，时间线列出通知了谁。
    pub fn with_notify(
        mut self,
        engine: Arc<NotifyEngine>,
        deliveries: Arc<dyn NotifyDeliveryRepository>,
    ) -> Self {
        self.notify_engine = Some(engine);
        self.deliveries = Some(deliveries);
        self
    }

    pub fn with_lifecycle_sink(mut self, sink: Arc<dyn AlertIncidentLifecycleSink>) -> Self {
        self.lifecycle_sink = Some(sink);
        self
    }

    /// 写一条备注。任何状态的事件都能写，并入别处的也能。
    pub async fn note(&self, actor: &Actor, incident_id: &Id, text: &str) -> Result<TimelineEntry> {
        let text = note_text(text)?;
        let incident = self.get(actor, incident_id).await?;

        let entry = TimelineEntry::new(
            &incident.org_id,
            &incident.id,
            actor.at,
            TimelineKind::Note,
            Some(actor.user_id.clone()),
            json!({ "text": text, "via": actor.via.as_str() }),
        );

        self.timeline.append(entry).await
    }

    /// 读出本组织的一个事件。
    async fn get(&self, actor: &Actor, incident_id: &Id) -> Result<Incident> {
        let incident = self.incidents.get(incident_id).await?;

        if incident.org_id != actor.org_id {
            return Err(Error::not_found("incident not found"));
        }

        Ok(incident)
    }

    /// 读出最新的事件交给 `change` 改，写回；期间被别人改过就重读再来。`change` 返回 `false` 时
    /// 不用改，原样返回。
    async fn modify<F>(&self, actor: &Actor, incident_id: &Id, mut change: F) -> Result<Incident>
    where
        F: FnMut(&mut Incident) -> Result<bool> + Send,
    {
        for _ in 0..WRITE_ATTEMPTS {
            let mut incident = self.get(actor, incident_id).await?;
            let status = incident.status;

            if !change(&mut incident)? {
                return Ok(incident);
            }

            if let Some(written) = self.incidents.update_if_status(incident, &[status]).await? {
                return Ok(written);
            }
        }

        Err(Error::conflict(
            "the incident kept changing while it was being updated; retry",
        ))
    }

    /// 记一条时间线，返回它的 ID 给直接通知当键（没记上时是一个新 ID）。失败只记日志：操作本身
    /// 已经生效。
    async fn record(
        &self,
        actor: &Actor,
        incident: &Incident,
        kind: TimelineKind,
        mut detail: Value,
    ) -> String {
        if let Value::Object(map) = &mut detail {
            map.insert("via".into(), Value::String(actor.via.as_str().into()));
        }

        let entry = TimelineEntry::new(
            &incident.org_id,
            &incident.id,
            actor.at,
            kind,
            actor.person(),
            detail,
        );

        let key = entry.id.to_string();

        if let Err(error) = self.timeline.append(entry).await {
            tracing::warn!(incident_id = %incident.id, %error, "incident timeline entry could not be written");
        }

        key
    }

    /// 直接通知这几个人。
    async fn page(&self, incident: &Incident, users: &[Id], reason: EscalationReason, key: &str) {
        let Some(engine) = &self.notify_engine else {
            return;
        };

        for user_id in users {
            let page = alert_page_dispatch(incident, user_id, reason, key);

            if let Err(error) = engine.enqueue_event(page).await {
                tracing::warn!(incident_id = %incident.id, %error, "incident notification could not be enqueued");
            }
        }
    }

    /// 立即通知事件当前这一步的对象（调度器下一轮会发同样 ID 的通知，不会重复）。
    async fn page_step(&self, incident: &Incident, policy: &EscalationPolicy) {
        if let Some(engine) = &self.notify_engine {
            dispatcher::enqueue_step(engine, incident, policy, incident.current_step as usize)
                .await;
        }
    }

    /// 事件的升级策略；默认策略的别名落到名为 `default` 的那条。
    async fn policy_of(&self, incident: &Incident) -> Result<Option<EscalationPolicy>> {
        let policies = self.policies.list(&incident.org_id).await?;
        Ok(EscalationPolicy::resolve(&policies, &incident.escalation_policy_id).cloned())
    }

    async fn emit(&self, incident: &Incident) {
        if let Some(sink) = &self.lifecycle_sink
            && let Err(error) = sink.on_incident_changed(incident).await
        {
            tracing::warn!(incident_id = %incident.id, %error, "alert incident lifecycle sink failed");
        }
    }
}

/// 只能处理进行中、没有并入别处的事件。
fn check_actionable(incident: &Incident) -> Result<()> {
    if incident.is_merged() {
        return Err(Error::conflict(
            "this incident was merged into another; act on that one",
        ));
    }

    if !incident.is_active() {
        return Err(Error::conflict("this incident is no longer active"));
    }

    Ok(())
}

/// 退回未确认：确认人与时间清掉，暂缓取消，当前这一步重新计时。
fn reopen(incident: &mut Incident, at: TimestampMicros) {
    incident.status = IncidentStatus::Open;
    incident.acknowledged_at = None;
    incident.acknowledged_by = None;
    incident.current_step_started_at = at;
    incident.handling.clear_snooze();
}
