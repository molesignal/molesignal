// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 修复动作：在事件上发起（建一个审批请求，记一条执行记录和时间线），审批通过后由审批的执行器调
//! [`RemediationService::execute`] 执行，执行后到点验证（事件恢复了没有，或规则回到健康没有）。

pub mod http;
pub mod kubernetes;

use std::{collections::BTreeMap, sync::Arc};

use serde_json::{Value, json};

use crate::{
    domain::alerting::{
        incident::{Incident, IncidentStatus},
        remediation::{
            RemediationAction, RemediationRepository, RemediationRun, RemediationRunStatus,
            RemediationTarget, VerifyBy, template_vars,
        },
        repositories::{AlertRuleRepository, IncidentRepository},
        rule::RuleState,
        timeline::{IncidentTimelineRepository, TimelineEntry, TimelineKind},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 一轮最多验证多少次执行。
const VERIFY_BATCH: i64 = 50;

pub struct RemediationService {
    repository: Arc<dyn RemediationRepository>,
    incidents: Arc<dyn IncidentRepository>,
    rules: Arc<dyn AlertRuleRepository>,
    timeline: Arc<dyn IncidentTimelineRepository>,
    external_url: String,
}

impl RemediationService {
    pub fn new(
        repository: Arc<dyn RemediationRepository>,
        incidents: Arc<dyn IncidentRepository>,
        rules: Arc<dyn AlertRuleRepository>,
        timeline: Arc<dyn IncidentTimelineRepository>,
        external_url: String,
    ) -> Self {
        Self {
            repository,
            incidents,
            rules,
            timeline,
            external_url: external_url.trim().trim_end_matches('/').to_string(),
        }
    }

    pub fn repository(&self) -> &Arc<dyn RemediationRepository> {
        &self.repository
    }

    /// 这个事件能用的动作：启用的，服务或团队对得上的。
    pub async fn applicable(&self, incident: &Incident) -> Result<Vec<RemediationAction>> {
        Ok(self
            .repository
            .list_actions(&incident.org_id)
            .await?
            .into_iter()
            .filter(|action| action.enabled && action.applies_to(incident))
            .collect())
    }

    /// 发起前的核对：动作在、启用、适用这个事件，参数填对了。返回动作与核对过的参数。
    pub async fn prepare(
        &self,
        incident: &Incident,
        action_id: &Id,
        given: &BTreeMap<String, String>,
    ) -> Result<(RemediationAction, BTreeMap<String, String>)> {
        let action = self
            .repository
            .get_action(&incident.org_id, action_id)
            .await?
            .ok_or_else(|| Error::not_found("remediation action not found"))?;

        if !action.enabled {
            return Err(Error::invalid("this remediation action is turned off"));
        }

        if !action.applies_to(incident) {
            return Err(Error::invalid(
                "this remediation action is not for this incident's services or team",
            ));
        }

        if matches!(
            incident.status,
            IncidentStatus::Resolved | IncidentStatus::Closed
        ) {
            return Err(Error::invalid("the incident is already resolved"));
        }

        let inputs = action.inputs(given)?;
        Ok((action, inputs))
    }

    /// 审批请求建好以后，记下这次发起。
    pub async fn record_request(
        &self,
        incident: &Incident,
        action: &RemediationAction,
        inputs: BTreeMap<String, String>,
        approval_id: Id,
        requested_by: &Id,
    ) -> Result<RemediationRun> {
        let now = TimestampMicros::now();
        let run = self
            .repository
            .create_run(RemediationRun {
                id: Id::new(),
                org_id: incident.org_id.clone(),
                incident_id: incident.id.clone(),
                action_id: action.id.clone(),
                action_name: action.name.clone(),
                approval_id,
                inputs,
                status: RemediationRunStatus::AwaitingApproval,
                output: String::new(),
                verify_at: None,
                verification: String::new(),
                requested_by: requested_by.clone(),
                executed_by: None,
                created_at: now,
                updated_at: now,
            })
            .await?;

        self.record(
            &run,
            TimelineKind::RemediationRequested,
            Some(requested_by.clone()),
            json!({ "runId": run.id, "action": run.action_name, "inputs": run.inputs }),
        )
        .await;

        Ok(run)
    }

    /// 审批通过后执行（审批的执行器调）。返回执行结果的一句话与写进执行记录的验证信息；失败时报错，
    /// 执行记录上同样记下。已经执行过的不再执行。
    pub async fn execute(
        &self,
        org_id: &Id,
        approval_id: &Id,
        executed_by: &Id,
    ) -> Result<(String, Value)> {
        let mut run = self
            .repository
            .find_run_by_approval(org_id, approval_id)
            .await?
            .ok_or_else(|| Error::not_found("this approval starts no remediation action"))?;

        if run.status != RemediationRunStatus::AwaitingApproval {
            return Ok((
                format!("remediation action already {}", run.status.as_str()),
                json!({ "verified": false, "runId": run.id }),
            ));
        }

        let incident = self.incidents.get(&run.incident_id).await?;
        let outcome = match self.repository.get_action(org_id, &run.action_id).await? {
            Some(action) => self
                .perform(&action, &incident, &run)
                .await
                .map(|output| (action, output)),
            None => Err(Error::not_found("the remediation action was deleted")),
        };

        let now = TimestampMicros::now();
        run.executed_by = Some(executed_by.clone());
        run.updated_at = now;

        match outcome {
            Ok((action, output)) => {
                run.output = output.clone();
                run.status = if action.verify_after_secs > 0 {
                    run.verify_at = Some(TimestampMicros(
                        now.0 + i64::from(action.verify_after_secs) * 1_000_000,
                    ));
                    RemediationRunStatus::Verifying
                } else {
                    RemediationRunStatus::Succeeded
                };

                let run = self.repository.update_run(run).await?;

                self.record(
                    &run,
                    TimelineKind::RemediationExecuted,
                    Some(executed_by.clone()),
                    json!({ "runId": run.id, "action": run.action_name, "status": "succeeded", "output": output }),
                )
                .await;

                Ok((
                    format!("{}: {output}", run.action_name),
                    json!({ "verified": false, "runId": run.id, "verifyAt": run.verify_at }),
                ))
            }
            Err(error) => {
                run.output = error.to_string();
                run.status = RemediationRunStatus::Failed;

                let run = self.repository.update_run(run).await?;

                self.record(
                    &run,
                    TimelineKind::RemediationExecuted,
                    Some(executed_by.clone()),
                    json!({ "runId": run.id, "action": run.action_name, "status": "failed", "output": run.output }),
                )
                .await;

                Err(error)
            }
        }
    }

    async fn perform(
        &self,
        action: &RemediationAction,
        incident: &Incident,
        run: &RemediationRun,
    ) -> Result<String> {
        let url = format!("{}/alerts/incidents/{}", self.external_url, incident.id);
        let vars = template_vars(incident, &url, &run.inputs);

        match &action.target {
            RemediationTarget::Http(target) => http::call(target, &vars).await,
            RemediationTarget::Kubernetes(target) => {
                let cluster = self
                    .repository
                    .get_cluster(&action.org_id, &target.cluster_id)
                    .await?
                    .ok_or_else(|| Error::not_found("the Kubernetes cluster was deleted"))?;

                kubernetes::run(&cluster, target, &vars).await
            }
        }
    }

    /// 审批被驳回、过期或撤销了：记成没执行。
    pub async fn decline(&self, mut run: RemediationRun, why: &str) -> Result<RemediationRun> {
        run.status = RemediationRunStatus::Declined;
        run.output = why.to_string();
        run.updated_at = TimestampMicros::now();

        self.repository.update_run(run).await
    }

    /// 验证到了时间的执行：事件恢复了没有，或规则回到健康没有。出错只记日志。
    pub async fn verify_due(&self, now: TimestampMicros) -> Result<()> {
        for run in self.repository.due_verifications(now, VERIFY_BATCH).await? {
            let run_id = run.id.clone();

            if let Err(error) = self.verify(run, now).await {
                tracing::warn!(%run_id, %error, "remediation verification failed");
            }
        }

        Ok(())
    }

    async fn verify(&self, mut run: RemediationRun, now: TimestampMicros) -> Result<()> {
        let incident = self.incidents.get(&run.incident_id).await?;
        let by = self
            .repository
            .get_action(&run.org_id, &run.action_id)
            .await?
            .map_or(VerifyBy::IncidentResolved, |action| action.verify_by);

        let passed = match by {
            VerifyBy::IncidentResolved => {
                matches!(
                    incident.status,
                    IncidentStatus::Resolved | IncidentStatus::Closed
                )
            }
            VerifyBy::RuleHealthy => self
                .rules
                .get(&incident.rule_id)
                .await
                .is_ok_and(|rule| rule.last_state == RuleState::Healthy),
        };

        run.status = if passed {
            RemediationRunStatus::Verified
        } else {
            RemediationRunStatus::VerificationFailed
        };
        run.verification = match (by, passed) {
            (VerifyBy::IncidentResolved, true) => "incident resolved",
            (VerifyBy::IncidentResolved, false) => "incident still open",
            (VerifyBy::RuleHealthy, true) => "rule healthy",
            (VerifyBy::RuleHealthy, false) => "rule not healthy",
        }
        .to_string();
        run.updated_at = now;

        let run = self.repository.update_run(run).await?;

        self.record(
            &run,
            TimelineKind::RemediationVerified,
            None,
            json!({
                "runId": run.id,
                "action": run.action_name,
                "passed": passed,
                "by": match by {
                    VerifyBy::IncidentResolved => "incident_resolved",
                    VerifyBy::RuleHealthy => "rule_healthy",
                },
            }),
        )
        .await;

        Ok(())
    }

    async fn record(
        &self,
        run: &RemediationRun,
        kind: TimelineKind,
        actor: Option<Id>,
        detail: Value,
    ) {
        let entry = TimelineEntry::new(
            &run.org_id,
            &run.incident_id,
            TimestampMicros::now(),
            kind,
            actor,
            detail,
        );

        if let Err(error) = self.timeline.append(entry).await {
            tracing::warn!(incident_id = %run.incident_id, %error, "remediation timeline entry could not be written");
        }
    }
}
