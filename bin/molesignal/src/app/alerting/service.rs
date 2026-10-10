// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use crate::{
    app::{
        alerting::{
            AlertIncidentLifecycleSink,
            realtime::{self, RealtimeAlerts},
        },
        notify::{
            ALERT_ACKNOWLEDGED_EVENT, ALERT_RESOLVED_EVENT, NotifyEngine, alert_dispatch,
            triggered_event_id,
        },
    },
    domain::alerting::{
        escalation::{DEFAULT_POLICY_ID, EscalationPolicy, EscalationStep, EscalationStepInput},
        incident::{Incident, IncidentStatus},
        repositories::{
            AlertRuleRepository, EscalationPolicyRepository, IncidentRepository, ScheduleRepository,
        },
        rule::AlertRule,
        schedule::Schedule,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct AlertingService {
    pub rules: Arc<dyn AlertRuleRepository>,
    pub incidents: Arc<dyn IncidentRepository>,
    pub schedules: Arc<dyn ScheduleRepository>,
    pub escalations: Arc<dyn EscalationPolicyRepository>,
    notify_engine: Option<Arc<NotifyEngine>>,
    lifecycle_sink: Option<Arc<dyn AlertIncidentLifecycleSink>>,
    /// 升级步骤省略 `ack_timeout_secs` 时的取值（`[alert].default_ack_timeout_secs`）。
    default_ack_timeout_secs: u32,
    /// 规则变更后通知实时告警匹配器重新加载。
    realtime_alerts: Option<Arc<RealtimeAlerts>>,
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
            lifecycle_sink: None,
            default_ack_timeout_secs: crate::config::AlertSettings::default()
                .default_ack_timeout_secs,
            realtime_alerts: None,
        }
    }

    pub fn with_realtime_alerts(mut self, alerts: Arc<RealtimeAlerts>) -> Self {
        self.realtime_alerts = Some(alerts);
        self
    }

    fn rules_changed(&self) {
        if let Some(alerts) = &self.realtime_alerts {
            alerts.rules_changed();
        }
    }

    pub fn with_default_ack_timeout_secs(mut self, secs: u32) -> Self {
        self.default_ack_timeout_secs = secs;
        self
    }

    /// 把提交的升级步骤落成带确定超时的步骤。
    pub fn escalation_steps(&self, inputs: Vec<EscalationStepInput>) -> Vec<EscalationStep> {
        inputs
            .into_iter()
            .map(|input| input.into_step(self.default_ack_timeout_secs))
            .collect()
    }

    pub fn with_notify_engine(mut self, notify_engine: Arc<NotifyEngine>) -> Self {
        self.notify_engine = Some(notify_engine);
        self
    }

    pub fn with_lifecycle_sink(mut self, sink: Arc<dyn AlertIncidentLifecycleSink>) -> Self {
        self.lifecycle_sink = Some(sink);
        self
    }

    // ---- AlertRule CRUD ----
    pub async fn create_rule(&self, rule: AlertRule) -> Result<AlertRule> {
        realtime::validate(&rule)?;
        self.ensure_policy_exists(&rule.org_id, &rule.escalation_policy_id)
            .await?;
        let created = self.rules.create(rule).await?;
        self.rules_changed();
        Ok(created)
    }

    pub async fn get_rule(&self, id: &Id) -> Result<AlertRule> {
        self.rules.get(id).await
    }

    pub async fn update_rule(&self, rule: AlertRule) -> Result<AlertRule> {
        realtime::validate(&rule)?;
        // 只校验被改动的引用：策略事后被删的规则仍可继续编辑其它字段。
        let existing = self.rules.get(&rule.id).await?;

        if existing.escalation_policy_id != rule.escalation_policy_id {
            self.ensure_policy_exists(&rule.org_id, &rule.escalation_policy_id)
                .await?;
        }

        let updated = self.rules.update(rule).await?;
        self.rules_changed();
        Ok(updated)
    }

    pub async fn list_rules(&self, org_id: &Id) -> Result<Vec<AlertRule>> {
        self.rules.list(org_id).await
    }

    pub async fn delete_rule(&self, id: &Id) -> Result<()> {
        self.rules.delete(id).await?;
        self.rules_changed();
        Ok(())
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

    /// 确认一个 `open` 事件，并把确认人记为负责人。
    ///
    /// 已确认的事件原样返回，保留最初的确认人与时间；已解决或关闭的事件返回冲突。
    pub async fn acknowledge(
        &self,
        incident_id: &Id,
        by: Id,
        at: TimestampMicros,
    ) -> Result<Incident> {
        let mut next = self.incidents.get(incident_id).await?;

        match next.status {
            IncidentStatus::Acknowledged => return Ok(next),
            IncidentStatus::Resolved | IncidentStatus::Closed => {
                return Err(Error::conflict("only an open incident can be acknowledged"));
            }
            IncidentStatus::Open => {}
        }

        next.status = IncidentStatus::Acknowledged;
        next.acknowledged_at = Some(at);
        next.acknowledged_by = Some(by.clone());

        if !next.assignees.contains(&by) {
            next.assignees.push(by);
        }

        let Some(incident) = self
            .incidents
            .update_if_status(next, &[IncidentStatus::Open])
            .await?
        else {
            // 读到状态之后被并发操作抢先：以库里的最新状态为准。
            let latest = self.incidents.get(incident_id).await?;

            return match latest.status {
                IncidentStatus::Acknowledged => Ok(latest),
                _ => Err(Error::conflict("only an open incident can be acknowledged")),
            };
        };

        self.emit_lifecycle(&incident).await;
        self.notify_transition(&incident, ALERT_ACKNOWLEDGED_EVENT, at)
            .await;
        Ok(incident)
    }

    /// 解决一个 `open` 或 `acknowledged` 事件。已解决或关闭的事件原样返回。
    pub async fn resolve(&self, incident_id: &Id, by: Id, at: TimestampMicros) -> Result<Incident> {
        let mut next = self.incidents.get(incident_id).await?;

        if matches!(
            next.status,
            IncidentStatus::Resolved | IncidentStatus::Closed
        ) {
            return Ok(next);
        }

        next.status = IncidentStatus::Resolved;
        next.resolved_at = Some(at);
        next.resolved_by = Some(by);

        let Some(incident) = self
            .incidents
            .update_if_status(next, &[IncidentStatus::Open, IncidentStatus::Acknowledged])
            .await?
        else {
            let latest = self.incidents.get(incident_id).await?;

            return match latest.status {
                IncidentStatus::Resolved | IncidentStatus::Closed => Ok(latest),
                _ => Err(Error::conflict(
                    "incident changed while it was being resolved; retry",
                )),
            };
        };

        self.emit_lifecycle(&incident).await;
        self.notify_transition(&incident, ALERT_RESOLVED_EVENT, at)
            .await;
        Ok(incident)
    }

    /// 状态迁移成功后的通知副作用：把触发事件标为已处理，并发出迁移事件。失败只记日志。
    async fn notify_transition(
        &self,
        incident: &Incident,
        event_type: &'static str,
        at: TimestampMicros,
    ) {
        let Some(engine) = &self.notify_engine else {
            return;
        };

        if let Err(error) = engine
            .acknowledge_event(&incident.org_id, &triggered_event_id(&incident.id), at)
            .await
        {
            tracing::warn!(
                incident_id = %incident.id,
                event_type,
                error = %error,
                "notify triggered event acknowledgement update failed"
            );
        }

        if let Err(error) = engine
            .enqueue_event(alert_dispatch(incident, event_type))
            .await
        {
            tracing::warn!(
                incident_id = %incident.id,
                event_type,
                error = %error,
                "notify alert transition event enqueue failed"
            );
        }
    }

    /// 规则引用的升级策略必须存在于同一组织。别名 `default` 放行：策略可以晚于规则创建。
    async fn ensure_policy_exists(&self, org_id: &Id, policy_id: &Id) -> Result<()> {
        if policy_id.0 == DEFAULT_POLICY_ID {
            return Ok(());
        }

        let policies = self.escalations.list(org_id).await?;

        if policies.iter().any(|policy| policy.id == *policy_id) {
            return Ok(());
        }

        Err(Error::invalid(format!(
            "escalation_policy_id `{policy_id}` does not match an escalation policy in this organization"
        )))
    }

    async fn emit_lifecycle(&self, incident: &Incident) {
        if let Some(sink) = &self.lifecycle_sink
            && let Err(error) = sink.on_incident_changed(incident).await
        {
            tracing::warn!(
                incident_id = %incident.id,
                error = %error,
                "alert Incident lifecycle sink failed"
            );
        }
    }
}
