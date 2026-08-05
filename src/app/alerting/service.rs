// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use crate::{
    app::notify::{
        ALERT_ACKNOWLEDGED_EVENT, ALERT_RESOLVED_EVENT, NotifyEngine, alert_dispatch,
        triggered_event_id,
    },
    domain::alerting::{
        escalation::EscalationPolicy,
        incident::{Incident, IncidentStatus},
        repositories::{
            AlertRuleRepository, EscalationPolicyRepository, IncidentRepository, ScheduleRepository,
        },
        rule::AlertRule,
        schedule::Schedule,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct AlertingService {
    pub rules: Arc<dyn AlertRuleRepository>,
    pub incidents: Arc<dyn IncidentRepository>,
    pub schedules: Arc<dyn ScheduleRepository>,
    pub escalations: Arc<dyn EscalationPolicyRepository>,
    notify_engine: Option<Arc<NotifyEngine>>,
}

impl AlertingService {
    pub fn new(
        rules: Arc<dyn AlertRuleRepository>,
        incidents: Arc<dyn IncidentRepository>,
        schedules: Arc<dyn ScheduleRepository>,
        escalations: Arc<dyn EscalationPolicyRepository>,
    ) -> Self {
        Self {
            rules,
            incidents,
            schedules,
            escalations,
            notify_engine: None,
        }
    }

    pub fn with_notify_engine(mut self, notify_engine: Arc<NotifyEngine>) -> Self {
        self.notify_engine = Some(notify_engine);
        self
    }

    // ---- AlertRule CRUD ----
    pub async fn create_rule(&self, rule: AlertRule) -> Result<AlertRule> {
        self.rules.create(rule).await
    }
    pub async fn get_rule(&self, id: &Id) -> Result<AlertRule> {
        self.rules.get(id).await
    }
    pub async fn update_rule(&self, rule: AlertRule) -> Result<AlertRule> {
        self.rules.update(rule).await
    }
    pub async fn list_rules(&self, org_id: &Id) -> Result<Vec<AlertRule>> {
        self.rules.list(org_id).await
    }
    pub async fn delete_rule(&self, id: &Id) -> Result<()> {
        self.rules.delete(id).await
    }

    // ---- Schedule ----
    pub async fn create_schedule(&self, s: Schedule) -> Result<Schedule> {
        self.schedules.create(s).await
    }
    pub async fn get_schedule(&self, id: &Id) -> Result<Schedule> {
        self.schedules.get(id).await
    }
    pub async fn list_schedules(&self, org_id: &Id) -> Result<Vec<Schedule>> {
        self.schedules.list(org_id).await
    }
    pub async fn update_schedule(&self, s: Schedule) -> Result<Schedule> {
        self.schedules.update(s).await
    }
    pub async fn delete_schedule(&self, id: &Id) -> Result<()> {
        self.schedules.delete(id).await
    }
    pub async fn who_is_on_call(
        &self,
        schedule_id: &Id,
        at: TimestampMicros,
    ) -> Result<Option<Id>> {
        let s = self.schedules.get(schedule_id).await?;
        Ok(s.who_is_on_call(at))
    }

    // ---- EscalationPolicy ----
    pub async fn create_policy(&self, p: EscalationPolicy) -> Result<EscalationPolicy> {
        self.escalations.create(p).await
    }
    pub async fn get_policy(&self, id: &Id) -> Result<EscalationPolicy> {
        self.escalations.get(id).await
    }
    pub async fn list_policies(&self, org_id: &Id) -> Result<Vec<EscalationPolicy>> {
        self.escalations.list(org_id).await
    }
    pub async fn update_policy(&self, p: EscalationPolicy) -> Result<EscalationPolicy> {
        self.escalations.update(p).await
    }
    pub async fn delete_policy(&self, id: &Id) -> Result<()> {
        self.escalations.delete(id).await
    }

    // ---- Incident lifecycle ----
    pub async fn get_incident(&self, id: &Id) -> Result<Incident> {
        self.incidents.get(id).await
    }
    pub async fn list_incidents_active(&self, org_id: &Id) -> Result<Vec<Incident>> {
        self.incidents.list_active(org_id).await
    }
    pub async fn list_incidents_by_status(
        &self,
        org_id: &Id,
        status: IncidentStatus,
    ) -> Result<Vec<Incident>> {
        self.incidents.list_by_status(org_id, status).await
    }

    pub async fn acknowledge(
        &self,
        incident_id: &Id,
        by: Id,
        at: TimestampMicros,
    ) -> Result<Incident> {
        let mut inc = self.incidents.get(incident_id).await?;
        inc.status = IncidentStatus::Acknowledged;
        inc.acknowledged_at = Some(at);
        inc.acknowledged_by = Some(by);
        let incident = self.incidents.update(inc).await?;
        if let Some(engine) = &self.notify_engine {
            if let Err(error) = engine
                .acknowledge_event(&incident.org_id, &triggered_event_id(&incident.id), at)
                .await
            {
                tracing::warn!(
                    incident_id = %incident.id,
                    error = %error,
                    "notify alert acknowledgement update failed"
                );
            }
            if let Err(error) = engine
                .enqueue_event(alert_dispatch(&incident, ALERT_ACKNOWLEDGED_EVENT))
                .await
            {
                tracing::warn!(
                    incident_id = %incident.id,
                    error = %error,
                    "notify alert acknowledgement event enqueue failed"
                );
            }
        }
        Ok(incident)
    }

    pub async fn resolve(&self, incident_id: &Id, by: Id, at: TimestampMicros) -> Result<Incident> {
        let mut inc = self.incidents.get(incident_id).await?;
        inc.status = IncidentStatus::Resolved;
        inc.resolved_at = Some(at);
        inc.resolved_by = Some(by);
        let incident = self.incidents.update(inc).await?;
        if let Some(engine) = &self.notify_engine {
            if let Err(error) = engine
                .acknowledge_event(&incident.org_id, &triggered_event_id(&incident.id), at)
                .await
            {
                tracing::warn!(
                    incident_id = %incident.id,
                    error = %error,
                    "notify alert resolved acknowledgement update failed"
                );
            }
            if let Err(error) = engine
                .enqueue_event(alert_dispatch(&incident, ALERT_RESOLVED_EVENT))
                .await
            {
                tracing::warn!(
                    incident_id = %incident.id,
                    error = %error,
                    "notify alert resolved event enqueue failed"
                );
            }
        }
        Ok(incident)
    }
}
