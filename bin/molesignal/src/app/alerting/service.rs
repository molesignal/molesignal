// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use crate::{
    app::{
        alerting::{
            AlertIncidentLifecycleSink,
            realtime::{self, RealtimeAlerts},
        },
        notify::{ALERT_ACKNOWLEDGED_EVENT, ALERT_RESOLVED_EVENT, NotifyEngine, alert_dispatch},
    },
    domain::alerting::{
        discovery,
        escalation::{DEFAULT_POLICY_ID, EscalationPolicy, EscalationStep, EscalationStepInput},
        incident::{Incident, IncidentStatus},
        repositories::{
            AlertRuleRepository, EscalationPolicyRepository, IncidentRepository, ScheduleRepository,
        },
        rule::{AlertQuery, AlertRule, validate_conditions},
        schedule::Schedule,
        timeline::{ActionVia, IncidentTimelineRepository, TimelineEntry, TimelineKind},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 写回一个事件最多试几次：每次落空都是别人刚改过，重读再改。
pub(crate) const WRITE_ATTEMPTS: usize = 5;

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
    /// 事件的时间线：确认、解决记在这里。
    timeline: Option<Arc<dyn IncidentTimelineRepository>>,
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
            timeline: None,
        }
    }

    pub fn with_timeline(mut self, timeline: Arc<dyn IncidentTimelineRepository>) -> Self {
        self.timeline = Some(timeline);
        self
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
    pub async fn create_rule(&self, mut rule: AlertRule) -> Result<AlertRule> {
        discovery::shape_rule(&mut rule)?;
        refuse_audit_trail(&rule)?;
        realtime::validate(&rule)?;
        validate_conditions(rule.kind, &rule.trigger, &rule.thresholds)?;
        self.ensure_policy_exists(&rule.org_id, &rule.escalation_policy_id)
            .await?;
        let created = self.rules.create(rule).await?;
        self.rules_changed();
        Ok(created)
    }

    pub async fn get_rule(&self, id: &Id) -> Result<AlertRule> {
        self.rules.get(id).await
    }

    pub async fn update_rule(&self, mut rule: AlertRule) -> Result<AlertRule> {
        discovery::shape_rule(&mut rule)?;
        refuse_audit_trail(&rule)?;
        realtime::validate(&rule)?;
        validate_conditions(rule.kind, &rule.trigger, &rule.thresholds)?;
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

    /// 不写库地检查一条要导入的规则，和保存时一样：统一自动发现规则的形状，校验实时规则与触发
    /// 条件，升级策略要在本组织。
    pub async fn check_rule(&self, rule: &mut AlertRule) -> Result<()> {
        discovery::shape_rule(rule)?;
        refuse_audit_trail(rule)?;
        realtime::validate(rule)?;
        validate_conditions(rule.kind, &rule.trigger, &rule.thresholds)?;
        self.ensure_policy_exists(&rule.org_id, &rule.escalation_policy_id)
            .await
    }

    /// 导入：检查过的一批规则一次写进去，都写进或都不写。
    pub async fn save_rules(&self, creates: Vec<AlertRule>, updates: Vec<AlertRule>) -> Result<()> {
        self.rules.save_all(creates, updates).await?;
        self.rules_changed();
        Ok(())
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
        via: ActionVia,
    ) -> Result<Incident> {
        for _ in 0..WRITE_ATTEMPTS {
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
            next.handling.clear_snooze();

            if !next.assignees.contains(&by) {
                next.assignees.push(by.clone());
            }

            // 读出之后被别人改过（调度器升级、评估器解决、别人确认）：重读再看。
            let Some(incident) = self
                .incidents
                .update_if_status(next, &[IncidentStatus::Open])
                .await?
            else {
                continue;
            };

            self.record(&incident, TimelineKind::Acknowledged, &by, at, via)
                .await;
            self.emit_lifecycle(&incident).await;
            self.notify_transition(&incident, ALERT_ACKNOWLEDGED_EVENT, at)
                .await;
            return Ok(incident);
        }

        Err(Error::conflict(
            "incident changed while it was being acknowledged; retry",
        ))
    }

    /// 解决一个 `open` 或 `acknowledged` 事件，并入它的事件一并解决。已解决或关闭的事件原样返回。
    pub async fn resolve(
        &self,
        incident_id: &Id,
        by: Id,
        at: TimestampMicros,
        via: ActionVia,
    ) -> Result<Incident> {
        let incident = self.resolve_one(incident_id, &by, at, via).await?;

        if let Some(incident) = &incident {
            self.resolve_merged(incident, &by, at, via).await;
        }

        match incident {
            Some(incident) => Ok(incident),
            None => self.incidents.get(incident_id).await,
        }
    }

    /// 解决一个事件，返回解决后的事件；它早已解决时返回 `None`。
    async fn resolve_one(
        &self,
        incident_id: &Id,
        by: &Id,
        at: TimestampMicros,
        via: ActionVia,
    ) -> Result<Option<Incident>> {
        for _ in 0..WRITE_ATTEMPTS {
            let mut next = self.incidents.get(incident_id).await?;
            let status = next.status;

            if matches!(status, IncidentStatus::Resolved | IncidentStatus::Closed) {
                return Ok(None);
            }

            next.status = IncidentStatus::Resolved;
            next.resolved_at = Some(at);
            next.resolved_by = Some(by.clone());
            next.handling.clear_snooze();

            let Some(incident) = self.incidents.update_if_status(next, &[status]).await? else {
                continue;
            };

            self.record(&incident, TimelineKind::Resolved, by, at, via)
                .await;
            self.emit_lifecycle(&incident).await;
            self.notify_transition(&incident, ALERT_RESOLVED_EVENT, at)
                .await;
            return Ok(Some(incident));
        }

        Err(Error::conflict(
            "incident changed while it was being resolved; retry",
        ))
    }

    /// 主事件解决了：并入它、还没解决的事件一并解决。失败只记日志。
    async fn resolve_merged(
        &self,
        target: &Incident,
        by: &Id,
        at: TimestampMicros,
        via: ActionVia,
    ) {
        let merged = match self
            .incidents
            .list_merged_into(&target.org_id, &target.id)
            .await
        {
            Ok(merged) => merged,
            Err(error) => {
                tracing::warn!(incident_id = %target.id, %error, "merged incidents could not be listed");
                return;
            }
        };

        for incident in merged.into_iter().filter(Incident::is_active) {
            if let Err(error) = self.resolve_one(&incident.id, by, at, via).await {
                tracing::warn!(
                    incident_id = %incident.id,
                    target_id = %target.id,
                    %error,
                    "merged incident could not be resolved with its target"
                );
            }
        }
    }

    /// 在时间线上记下有人确认或解决了事件。失败只记日志：操作本身已经生效。
    async fn record(
        &self,
        incident: &Incident,
        kind: TimelineKind,
        by: &Id,
        at: TimestampMicros,
        via: ActionVia,
    ) {
        let Some(timeline) = &self.timeline else {
            return;
        };

        let entry = TimelineEntry::new(
            &incident.org_id,
            &incident.id,
            at,
            kind,
            Some(by.clone()),
            serde_json::json!({ "via": via.as_str() }),
        );

        if let Err(error) = timeline.append(entry).await {
            tracing::warn!(incident_id = %incident.id, %error, "incident timeline entry could not be written");
        }
    }

    /// 状态迁移成功后的通知副作用：把触发与各次升级的通知标为已处理（不再有后续步骤和来电），并发出
    /// 迁移事件。失败只记日志。
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
            .acknowledge_alert(&incident.org_id, &incident.id, at)
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

/// Whether a rule's query reads the audit trail (`_audit`).
fn reads_audit_trail(query: &AlertQuery) -> bool {
    query
        .stream
        .as_ref()
        .is_some_and(|stream| stream.name == crate::domain::stream::AUDIT_STREAM)
        || crate::infra::query::grants::statement_names_audit(&query.statement)
}

/// A rule is evaluated in the background, where nobody's `audit.read` applies: one that reads the
/// audit trail could never see it, and would show healthy for ever.
fn refuse_audit_trail(rule: &AlertRule) -> Result<()> {
    if reads_audit_trail(&rule.query) {
        return Err(Error::invalid(
            "an alert rule cannot read `_audit`: rules are evaluated in the background, where \
             nobody's audit.read applies",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_that_reads_the_audit_trail_is_told_apart() {
        let query =
            |value: serde_json::Value| -> AlertQuery { serde_json::from_value(value).unwrap() };
        let on = |statement: &str, stream: &str| {
            query(serde_json::json!({
                "language": "sql",
                "statement": statement,
                "periodSecs": 60,
                "stream": { "name": stream, "streamType": "logs" },
            }))
        };

        assert!(reads_audit_trail(&on(
            "SELECT count(*) FROM _audit",
            "_audit"
        )));
        assert!(reads_audit_trail(&on("SELECT count(*) FROM _audit", "app")));
        assert!(reads_audit_trail(&on("SELECT 1", "_audit")));
        assert!(reads_audit_trail(&on(
            "SELECT count(*) FROM app WHERE a IN (SELECT action FROM _audit)",
            "app"
        )));
        assert!(!reads_audit_trail(&on("SELECT count(*) FROM app", "app")));
    }
}
