// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 自动化的触发循环：定时规则到点运行、新事件和新发现触发规则、给没写回的运行收尾、清理旧记录。
//!
//! 提供完整 HTTP 的节点各起一份，靠数据库协调：定时规则用 SKIP LOCKED 领走；事件游标比较后
//! 推进，只有推进成功的节点处理那一批事件；运行记录只结束一次。

use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    agent::automations::{
        Automation, AutomationTrigger, IncidentFacts, KEPT_RUNS_PER_AUTOMATION,
        MERGE_WINDOW_MICROS, TriggerKind, reasons,
    },
    api::{
        AppState,
        http::routes::agent::automations::{Start, reconcile, start},
    },
    app::automations::schedule,
    domain::alerting::{
        discovery::{Finding, FindingFilter as Findings},
        incident::Incident,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 多久看一次到点的规则与新事件。
const TICK: Duration = Duration::from_secs(30);
/// 领走定时规则时先把下一次推后这么久：节点在算出真正的下一次之前没了，规则晚一点照常运行。
const HOLD_MICROS: i64 = 15 * 60 * 1_000_000;
/// 一次最多开始这么多次定时运行，每条事件规则一次最多看这么多个新事件。
const BATCH: i64 = 20;
/// 多久清理一次旧的运行记录。
const PURGE_EVERY: Duration = Duration::from_secs(60 * 60);

pub struct AutomationWorker {
    state: AppState,
}

impl AutomationWorker {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        let worker = Arc::new(self);

        tokio::spawn(async move {
            let mut purged_at = tokio::time::Instant::now();

            loop {
                tokio::time::sleep(TICK).await;

                let purge = purged_at.elapsed() >= PURGE_EVERY;

                if purge {
                    purged_at = tokio::time::Instant::now();
                }

                worker.tick(purge).await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.agent_automations",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "agent_automations")
    )]
    async fn tick(&self, purge: bool) {
        if let Err(error) = self.schedules().await {
            tracing::warn!(%error, "scheduled automations could not be started");
        }

        if let Err(error) = self.incidents().await {
            tracing::warn!(%error, "incident automations could not be started");
        }

        if let Err(error) = reconcile(&self.state).await {
            tracing::warn!(%error, "automation runs could not be settled");
        }

        if purge {
            match self
                .state
                .agent
                .automations
                .purge_runs(KEPT_RUNS_PER_AUTOMATION)
                .await
            {
                Ok(0) => {}
                Ok(purged) => tracing::debug!(purged, "old automation runs purged"),
                Err(error) => tracing::warn!(%error, "old automation runs could not be purged"),
            }
        }
    }

    async fn schedules(&self) -> Result<()> {
        let repo = &self.state.agent.automations;
        let now = TimestampMicros::now();
        let hold = TimestampMicros(now.0 + HOLD_MICROS);

        for automation in repo.claim_due(now, hold, BATCH).await? {
            let next = match &automation.trigger {
                AutomationTrigger::Schedule { cron, timezone } => {
                    schedule::next_after(cron, timezone, now).unwrap_or_else(|error| {
                        tracing::warn!(automation = %automation.id.0, %error, "an automation's schedule cannot be read");
                        None
                    })
                }
                _ => None,
            };

            repo.set_next_run(&automation.id, next).await?;
            self.launch(&automation, TriggerKind::Schedule, None).await;
        }

        Ok(())
    }

    async fn incidents(&self) -> Result<()> {
        let repo = &self.state.agent.automations;
        let rules = repo.incident_rules().await?;
        let mut by_org: HashMap<Id, Vec<Automation>> = HashMap::new();

        for rule in rules {
            by_org.entry(rule.org_id.clone()).or_default().push(rule);
        }

        let now = TimestampMicros::now();

        for (org_id, rules) in by_org {
            let (finding_rules, rules): (Vec<Automation>, Vec<Automation>) = rules
                .into_iter()
                .partition(|rule| matches!(rule.trigger, AutomationTrigger::Finding(_)));

            if !finding_rules.is_empty() {
                self.findings(&org_id, &finding_rules, now).await?;
            }

            if rules.is_empty() {
                continue;
            }

            // 这个组织里最早的游标之后的事件，一次读出来给各条规则分。
            let since = rules
                .iter()
                .map(|rule| rule.incident_cursor.unwrap_or(now).0)
                .min()
                .unwrap_or(now.0);

            let mut incidents = self
                .state
                .alerting
                .service
                .incidents
                .list_since(&org_id, TimestampMicros(since))
                .await?;

            incidents.sort_by_key(|incident| incident.created_at.0);

            for rule in rules {
                self.incident_rule(&rule, &incidents, now).await?;
            }
        }

        Ok(())
    }

    async fn incident_rule(
        &self,
        rule: &Automation,
        incidents: &[Incident],
        now: TimestampMicros,
    ) -> Result<()> {
        let AutomationTrigger::Incident(filter) = &rule.trigger else {
            return Ok(());
        };

        let cursor = rule.incident_cursor.unwrap_or(now);

        let fresh = incidents
            .iter()
            .filter(|incident| incident.created_at.0 > cursor.0)
            .take(usize::try_from(BATCH).unwrap_or(20))
            .collect::<Vec<_>>();

        let Some(last) = fresh.last() else {
            return Ok(());
        };

        // 别的节点已经推进过游标：这一批由它处理。
        if !self
            .state
            .agent
            .automations
            .advance_incident_cursor(&rule.id, rule.incident_cursor, last.created_at)
            .await?
        {
            return Ok(());
        }

        for incident in fresh {
            if !filter.matches(IncidentFacts::of(incident)) {
                continue;
            }

            let analyzed = self
                .state
                .agent
                .automations
                .analyzed(
                    &rule.id,
                    &incident.id,
                    &incident.fingerprint,
                    TimestampMicros(now.0 - MERGE_WINDOW_MICROS),
                )
                .await?;

            if analyzed {
                self.skip_analyzed(rule, incident).await;
                continue;
            }

            self.launch(rule, TriggerKind::Incident, Some(incident.clone()))
                .await;
        }

        Ok(())
    }

    /// 同一组事件刚分析过：记一条跳过的运行，运行记录里看得到为什么没分析。
    async fn skip_analyzed(&self, rule: &Automation, incident: &Incident) {
        let now = TimestampMicros::now();

        let record = crate::agent::automations::AutomationRun {
            id: Id::new(),
            org_id: rule.org_id.clone(),
            automation_id: rule.id.clone(),
            trigger: TriggerKind::Incident,
            status: crate::agent::automations::AutomationRunStatus::Skipped,
            reason: Some(reasons::ALREADY_ANALYZED.into()),
            chat_id: None,
            run_id: None,
            incident_id: Some(incident.id.clone()),
            group_key: Some(incident.fingerprint.clone()),
            input: Some(incident.summary.clone()),
            summary: None,
            outputs: serde_json::json!({}),
            started_by: None,
            created_at: now,
            finished_at: Some(now),
        };

        if let Err(error) = self.state.agent.automations.create_run(record).await {
            tracing::warn!(automation = %rule.id.0, %error, "a skipped automation run could not be recorded");
        }
    }

    /// 发现触发的规则：组织里游标之后新开的发现，满足筛选的各调查一次。
    async fn findings(
        &self,
        org_id: &Id,
        rules: &[Automation],
        now: TimestampMicros,
    ) -> Result<()> {
        let since = rules
            .iter()
            .map(|rule| rule.incident_cursor.unwrap_or(now).0)
            .min()
            .unwrap_or(now.0);

        let mut findings: Vec<Finding> = self
            .state
            .alerting
            .discovery
            .list(
                org_id,
                &Findings {
                    open: Some(true),
                    kind: None,
                    since: Some(TimestampMicros(since)),
                    limit: 200,
                },
            )
            .await?
            .into_iter()
            .filter(|finding| finding.created_at.0 > since)
            .collect();

        findings.sort_by_key(|finding| finding.created_at.0);

        for rule in rules {
            let AutomationTrigger::Finding(filter) = &rule.trigger else {
                continue;
            };

            let cursor = rule.incident_cursor.unwrap_or(now);
            let fresh: Vec<&Finding> = findings
                .iter()
                .filter(|finding| finding.created_at.0 > cursor.0)
                .take(usize::try_from(BATCH).unwrap_or(20))
                .collect();

            let Some(last) = fresh.last() else {
                continue;
            };

            // 别的节点已经推进过游标：这一批由它处理。
            if !self
                .state
                .agent
                .automations
                .advance_incident_cursor(&rule.id, rule.incident_cursor, last.created_at)
                .await?
            {
                continue;
            }

            for finding in fresh.into_iter().filter(|finding| filter.matches(finding)) {
                // 同一条发现只调查一次。
                let analyzed = self
                    .state
                    .agent
                    .automations
                    .analyzed(&rule.id, &finding.id, &finding.id.0, TimestampMicros(0))
                    .await?;

                if !analyzed {
                    self.start(rule, TriggerKind::Finding, None, Some(finding.clone()))
                        .await;
                }
            }
        }

        Ok(())
    }

    async fn launch(&self, rule: &Automation, trigger: TriggerKind, incident: Option<Incident>) {
        self.start(rule, trigger, incident, None).await;
    }

    async fn start(
        &self,
        rule: &Automation,
        trigger: TriggerKind,
        incident: Option<Incident>,
        finding: Option<Finding>,
    ) {
        let started = start(
            &self.state,
            rule,
            Start {
                trigger,
                text: None,
                incident,
                finding,
                started_by: None,
            },
        )
        .await;

        if let Err(error) = started {
            tracing::warn!(automation = %rule.id.0, %error, "an automation run could not be started");
        }
    }
}
