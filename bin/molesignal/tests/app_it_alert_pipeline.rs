// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Alert pipeline 端到端（in-mem repos，无 docker）：
//!
//! 1. 1 条 rule，trigger threshold=10, for_periods=2
//! 2. Mock query engine 第一次返 value=15 → matched=true → state=1（无 incident，未达 for_periods）
//! 3. 第二次返 value=15 → matched=true → state=2（创建 incident）
//! 4. 第三次返 value=5 → matched=false → incident Resolved + state reset
//!
//! 验证：RuleEvaluator 的 `consecutive_matches` 累积 + open/resolve 生命周期 +
//! `eval_state_repo.reset` 在 resolve 时被调。

use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use molesignal::{
    app::{
        alerting::{AlertingService, RuleEvaluator},
        cluster::{ClusterRegistry, PeerInfo, PeerRole, WorkOwnership},
        synthetics::NoDataChange,
    },
    domain::{
        alerting::{
            anomaly::AnomalyParams,
            escalation::{DEFAULT_POLICY_ID, EscalationPolicy},
            incident::{Incident, IncidentStatus, Severity},
            incident_group::{IncidentGroup, IncidentGroupRepository, IncidentGroupState},
            repositories::{
                AlertRuleEvalState, AlertRuleEvalStateRepository, AlertRuleRepository,
                EscalationPolicyRepository, IncidentRepository, ScheduleRepository,
            },
            rule::{
                AlertQuery, AlertRule, AlertRuleKind, AlertTrigger, ComparisonOp, MAX_ALERT_GROUPS,
                NoDataAction, NoDataCondition, RuleState, SeverityThreshold,
            },
            schedule::Schedule,
            semantic_group::{LabelMatcher, MatchOp, SemanticGroup, SemanticGroupRepository},
            timeline::ActionVia,
        },
        query::{PromqlEngine, QueryEngine, QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::StreamType,
        synthetics::{
            MonitorKind, MonitorLifecycle, MonitorRevision, MonitorSchedule, MonitorSpec,
            MonitorState, MultiLocationPolicy, SyntheticMonitor, SyntheticStateTransition,
        },
    },
    infra::query::language_router::LanguageRoutedEngine,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

// ---- in-mem AlertRuleRepository ----
struct InMemRules {
    inner: StdMutex<Vec<AlertRule>>,
}
#[async_trait]
impl AlertRuleRepository for InMemRules {
    async fn create(&self, r: AlertRule) -> Result<AlertRule> {
        self.inner.lock().unwrap().push(r.clone());
        Ok(r)
    }
    async fn update(&self, r: AlertRule) -> Result<AlertRule> {
        let mut g = self.inner.lock().unwrap();
        if let Some(existing) = g.iter_mut().find(|x| x.id == r.id) {
            *existing = r.clone();
        }
        Ok(r)
    }
    async fn get(&self, id: &Id) -> Result<AlertRule> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .find(|r| &r.id == id)
            .cloned()
            .ok_or_else(|| molesignal::shared::Error::not_found("rule"))
    }
    async fn list(&self, _org: &Id) -> Result<Vec<AlertRule>> {
        Ok(self.inner.lock().unwrap().clone())
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
    async fn list_enabled(&self) -> Result<Vec<AlertRule>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.enabled)
            .cloned()
            .collect())
    }
    async fn record_evaluation(
        &self,
        id: &Id,
        evaluated_at: TimestampMicros,
        state: RuleState,
    ) -> Result<()> {
        if let Some(rule) = self.inner.lock().unwrap().iter_mut().find(|r| &r.id == id) {
            rule.last_eval_at = Some(evaluated_at);
            rule.last_state = state;
        }
        Ok(())
    }
}

// ---- in-mem IncidentRepository ----
#[derive(Default)]
struct InMemIncidents {
    inner: StdMutex<Vec<Incident>>,
}
#[async_trait]
impl IncidentRepository for InMemIncidents {
    async fn create(&self, i: Incident) -> Result<Incident> {
        self.inner.lock().unwrap().push(i.clone());
        Ok(i)
    }
    async fn update(&self, i: Incident) -> Result<Incident> {
        let mut g = self.inner.lock().unwrap();
        if let Some(e) = g.iter_mut().find(|x| x.id == i.id) {
            *e = i.clone();
        }
        Ok(i)
    }
    async fn update_if_status(
        &self,
        i: Incident,
        expected: &[IncidentStatus],
    ) -> Result<Option<Incident>> {
        let mut g = self.inner.lock().unwrap();
        match g.iter_mut().find(|x| x.id == i.id) {
            Some(existing)
                if expected.contains(&existing.status) && existing.revision == i.revision =>
            {
                let written = Incident {
                    revision: i.revision + 1,
                    ..i
                };
                *existing = written.clone();
                Ok(Some(written))
            }
            _ => Ok(None),
        }
    }
    async fn list_merged_into(&self, org: &Id, target: &Id) -> Result<Vec<Incident>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|i| &i.org_id == org && i.handling.merged_into.as_ref() == Some(target))
            .cloned()
            .collect())
    }
    async fn get(&self, id: &Id) -> Result<Incident> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .find(|i| &i.id == id)
            .cloned()
            .ok_or_else(|| molesignal::shared::Error::not_found("incident"))
    }
    async fn list_active(&self, org: &Id) -> Result<Vec<Incident>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|i| {
                &i.org_id == org
                    && matches!(
                        i.status,
                        IncidentStatus::Open | IncidentStatus::Acknowledged
                    )
            })
            .cloned()
            .collect())
    }
    async fn list_active_for_rule(&self, org: &Id, rule: &Id) -> Result<Vec<Incident>> {
        Ok(self
            .list_active(org)
            .await?
            .into_iter()
            .filter(|i| &i.rule_id == rule)
            .collect())
    }
    async fn find_by_fingerprint(&self, org: &Id, fp: &str) -> Result<Option<Incident>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .find(|i| &i.org_id == org && i.fingerprint == fp)
            .cloned())
    }
    async fn list_by_status(&self, org: &Id, s: IncidentStatus) -> Result<Vec<Incident>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|i| &i.org_id == org && i.status == s)
            .cloned()
            .collect())
    }
    async fn list_since(
        &self,
        org: &Id,
        since: molesignal::shared::time::TimestampMicros,
    ) -> Result<Vec<Incident>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|i| &i.org_id == org && i.created_at.0 >= since.0)
            .cloned()
            .collect())
    }
}

// ---- in-mem AlertRuleEvalStateRepository（键是 (rule_id, group_key)）----
#[derive(Default)]
struct InMemEvalState {
    inner: StdMutex<HashMap<(String, String), AlertRuleEvalState>>,
}

fn state_key(rule_id: &Id, group_key: &str) -> (String, String) {
    (rule_id.0.clone(), group_key.to_string())
}
#[async_trait]
impl AlertRuleEvalStateRepository for InMemEvalState {
    async fn claim_cycle(
        &self,
        rule_id: &Id,
        eval_at: TimestampMicros,
        min_gap_micros: i64,
    ) -> Result<bool> {
        let mut g = self.inner.lock().unwrap();
        let state = g.entry(state_key(rule_id, "")).or_insert_with(|| {
            AlertRuleEvalState::fresh(
                rule_id,
                "",
                Default::default(),
                TimestampMicros(i64::MIN / 2),
            )
        });
        if state.last_eval_at.0 > eval_at.0 - min_gap_micros {
            return Ok(false);
        }
        state.last_eval_at = eval_at;
        Ok(true)
    }
    async fn get(&self, rule_id: &Id, group_key: &str) -> Result<Option<AlertRuleEvalState>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .get(&state_key(rule_id, group_key))
            .cloned())
    }
    async fn list_groups(&self, rule_id: &Id) -> Result<Vec<AlertRuleEvalState>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|((rule, group), _)| rule == &rule_id.0 && !group.is_empty())
            .map(|(_, state)| state.clone())
            .collect())
    }
    async fn upsert_state(&self, state: AlertRuleEvalState) -> Result<AlertRuleEvalState> {
        self.inner
            .lock()
            .unwrap()
            .insert(state_key(&state.rule_id, &state.group_key), state.clone());
        Ok(state)
    }
    async fn reset(&self, rule_id: &Id, group_key: &str) -> Result<()> {
        if let Some(s) = self
            .inner
            .lock()
            .unwrap()
            .get_mut(&state_key(rule_id, group_key))
        {
            s.consecutive_matches = 0;
            s.last_matched = false;
            s.severity_streaks.clear();
        }
        Ok(())
    }
    async fn forget_groups(&self, rule_id: &Id, group_keys: &[String]) -> Result<()> {
        self.inner.lock().unwrap().retain(|(rule, group), _| {
            rule != &rule_id.0 || group.is_empty() || !group_keys.contains(group)
        });
        Ok(())
    }
}

// ---- Mock QueryEngine：每次 execute 按计数器返回不同 value ----
struct ScriptedEngine {
    values: StdMutex<Vec<f64>>,
}
#[async_trait]
impl QueryEngine for ScriptedEngine {
    async fn execute(&self, _req: QueryRequest) -> Result<QueryResult> {
        let val = self.values.lock().unwrap().pop().unwrap_or(0.0); // 用栈顶（脚本反序压入）
        Ok(QueryResult {
            columns: vec!["v".into()],
            rows: vec![vec![serde_json::json!(val)]],
            scanned_rows: 1,
            took_ms: 0,
            federation: None,
        })
    }
}

fn sample_rule() -> AlertRule {
    AlertRule {
        id: Id::from_string("rule-1"),
        org_id: Id::from_string("org-a"),
        name: "high latency".into(),
        description: String::new(),
        enabled: true,
        kind: AlertRuleKind::Scheduled,
        query: AlertQuery {
            language: QueryLanguage::Sql,
            statement: "SELECT max(latency) FROM logs".into(),
            period_secs: 60,
            stream: Some(StreamHint {
                name: "logs".into(),
                stream_type: StreamType::LOGS,
            }),
            apm: None,
            rum: None,
            group_by: Vec::new(),
            composite: None,
            forecast: None,
            outlier: None,
            heartbeat: None,
            delay_secs: None,
            discovery: None,
            home_cluster: None,
        },
        anomaly_params: None,
        trigger: AlertTrigger {
            operator: ComparisonOp::Gt,
            threshold: 10.0,
            for_periods: 2,
            silence_secs: 0,
            no_data: None,
            recovery: None,
            priority: None,
            urgency: None,
        },
        thresholds: vec![],
        severity: None,
        escalation_policy_id: Id::from_string("pol-1"),
        labels: BTreeMap::new(),
        annotations: BTreeMap::new(),
        last_eval_at: None,
        last_state: RuleState::default(),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

#[tokio::test]
async fn evaluator_open_then_resolve_lifecycle() {
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![sample_rule()]),
    });
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let eval_state: Arc<InMemEvalState> = Arc::new(InMemEvalState::default());
    // 脚本压栈顺序与读取相反：第一次 pop → 15.0；第二次 → 15.0；第三次 → 5.0
    let engine = Arc::new(ScriptedEngine {
        values: StdMutex::new(vec![5.0, 15.0, 15.0]),
    });
    let evaluator = RuleEvaluator::new(
        rules.clone() as Arc<dyn AlertRuleRepository>,
        incidents.clone() as Arc<dyn IncidentRepository>,
        eval_state.clone() as Arc<dyn AlertRuleEvalStateRepository>,
        engine,
        10,
    );

    let now1 = TimestampMicros::now();
    evaluator.tick(now1).await.unwrap();
    // 第一次：matched=true → consecutive=1, no incident（for_periods=2）
    assert_eq!(
        eval_state
            .get(&Id::from_string("rule-1"), "")
            .await
            .unwrap()
            .unwrap()
            .consecutive_matches,
        1
    );
    assert_eq!(incidents.inner.lock().unwrap().len(), 0);

    let now2 = TimestampMicros(now1.0 + 60_000_000);
    evaluator.tick(now2).await.unwrap();
    // 第二次：matched=true → consecutive=2，open incident
    assert_eq!(
        eval_state
            .get(&Id::from_string("rule-1"), "")
            .await
            .unwrap()
            .unwrap()
            .consecutive_matches,
        2
    );
    {
        let g = incidents.inner.lock().unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].status, IncidentStatus::Open);
    }

    let now3 = TimestampMicros(now2.0 + 60_000_000);
    evaluator.tick(now3).await.unwrap();
    // 第三次：matched=false → incident Resolved + state reset
    {
        let g = incidents.inner.lock().unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].status, IncidentStatus::Resolved);
    }
    let s = eval_state
        .get(&Id::from_string("rule-1"), "")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(s.consecutive_matches, 0);
    assert!(!s.last_matched);
}

// ───────────────────────── anomaly (kind = Anomaly) ─────────────────────────

/// kind = Anomaly 的样本规则：MAD detector，lookback_days=5，for_periods=1。
/// trigger.threshold/operator 对 anomaly 不参与判定（MAD 决定 firing）。
fn anomaly_rule() -> AlertRule {
    AlertRule {
        id: Id::from_string("rule-anom"),
        org_id: Id::from_string("org-a"),
        name: "latency anomaly".into(),
        description: String::new(),
        enabled: true,
        kind: AlertRuleKind::Anomaly,
        query: AlertQuery {
            language: QueryLanguage::Sql,
            statement: "SELECT avg(latency) FROM logs".into(),
            period_secs: 60,
            stream: Some(StreamHint {
                name: "logs".into(),
                stream_type: StreamType::LOGS,
            }),
            apm: None,
            rum: None,
            group_by: Vec::new(),
            composite: None,
            forecast: None,
            outlier: None,
            heartbeat: None,
            delay_secs: None,
            discovery: None,
            home_cluster: None,
        },
        anomaly_params: Some(AnomalyParams {
            algorithm: "mad".into(),
            lookback_days: 5,
            k: 3.0,
            alpha: 0.3,
            weekly_seasonality: false,
        }),
        trigger: AlertTrigger {
            operator: ComparisonOp::Gt,
            threshold: 0.0,
            for_periods: 1,
            silence_secs: 0,
            no_data: None,
            recovery: None,
            priority: None,
            urgency: None,
        },
        thresholds: vec![],
        severity: None,
        escalation_policy_id: Id::from_string("pol-1"),
        labels: BTreeMap::new(),
        annotations: BTreeMap::new(),
        last_eval_at: None,
        last_state: RuleState::default(),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

#[tokio::test]
async fn anomaly_rule_fires_on_outlier() {
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![anomaly_rule()]),
    });
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let eval_state: Arc<InMemEvalState> = Arc::new(InMemEvalState::default());
    // execute 调用序（每 tick）：current 先取一次，再按 lookback_days 取 5 个历史窗口。
    // ScriptedEngine.pop() 取栈顶 → 末元素先返：current=500（离群），历史={48..52}。
    let engine = Arc::new(ScriptedEngine {
        values: StdMutex::new(vec![48.0, 49.0, 50.0, 51.0, 52.0, 500.0]),
    });
    let evaluator = RuleEvaluator::new(
        rules.clone() as Arc<dyn AlertRuleRepository>,
        incidents.clone() as Arc<dyn IncidentRepository>,
        eval_state.clone() as Arc<dyn AlertRuleEvalStateRepository>,
        engine,
        10,
    );

    evaluator.tick(TimestampMicros::now()).await.unwrap();
    let g = incidents.inner.lock().unwrap();
    assert_eq!(g.len(), 1, "MAD outlier should open exactly one incident");
    assert_eq!(g[0].status, IncidentStatus::Open);
    assert_eq!(g[0].rule_id, Id::from_string("rule-anom"));
}

/// 当前窗口成功、历史窗口全部超时/出错的引擎：复现 review high 的故障注入。
/// 区分依据：当前窗口 `time_range.end == boundary`，历史窗口 `end < boundary`。
struct HistoryFailEngine {
    boundary: i64,
    current: f64,
}
#[async_trait]
impl QueryEngine for HistoryFailEngine {
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        if req.time_range.end.0 < self.boundary {
            return Err(molesignal::shared::Error::internal(
                "transient history failure",
            ));
        }
        Ok(QueryResult {
            columns: vec!["v".into()],
            rows: vec![vec![serde_json::json!(self.current)]],
            scanned_rows: 1,
            took_ms: 0,
            federation: None,
        })
    }
}

#[tokio::test]
async fn anomaly_transient_history_failure_does_not_resolve_open_incident() {
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![anomaly_rule()]),
    });
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let eval_state: Arc<InMemEvalState> = Arc::new(InMemEvalState::default());

    // tick1：好引擎，outlier → 开 incident。
    let now1 = TimestampMicros::now();
    let good = Arc::new(ScriptedEngine {
        values: StdMutex::new(vec![48.0, 49.0, 50.0, 51.0, 52.0, 500.0]),
    });
    RuleEvaluator::new(
        rules.clone() as Arc<dyn AlertRuleRepository>,
        incidents.clone() as Arc<dyn IncidentRepository>,
        eval_state.clone() as Arc<dyn AlertRuleEvalStateRepository>,
        good,
        10,
    )
    .tick(now1)
    .await
    .unwrap();
    assert_eq!(
        incidents.inner.lock().unwrap()[0].status,
        IncidentStatus::Open
    );

    // tick2：当前窗口成功、历史窗口全部出错 → inconclusive → 绝不能 resolve 掉 incident。
    let now2 = TimestampMicros(now1.0 + 60_000_000);
    let flaky = Arc::new(HistoryFailEngine {
        boundary: now2.0,
        current: 50.0,
    });
    RuleEvaluator::new(
        rules.clone() as Arc<dyn AlertRuleRepository>,
        incidents.clone() as Arc<dyn IncidentRepository>,
        eval_state.clone() as Arc<dyn AlertRuleEvalStateRepository>,
        flaky,
        10,
    )
    .tick(now2)
    .await
    .unwrap();
    let g = incidents.inner.lock().unwrap();
    assert_eq!(g.len(), 1);
    assert_eq!(
        g[0].status,
        IncidentStatus::Open,
        "inconclusive baseline (history fetch failed) must not resolve a firing incident"
    );
}

#[tokio::test]
async fn anomaly_rule_quiet_within_baseline() {
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![anomaly_rule()]),
    });
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let eval_state: Arc<InMemEvalState> = Arc::new(InMemEvalState::default());
    // current=51 紧贴基线（median 50，robust σ≈1.48 → 3σ≈4.45）→ 不触发。
    let engine = Arc::new(ScriptedEngine {
        values: StdMutex::new(vec![48.0, 49.0, 50.0, 51.0, 52.0, 51.0]),
    });
    let evaluator = RuleEvaluator::new(
        rules.clone() as Arc<dyn AlertRuleRepository>,
        incidents.clone() as Arc<dyn IncidentRepository>,
        eval_state.clone() as Arc<dyn AlertRuleEvalStateRepository>,
        engine,
        10,
    );

    evaluator.tick(TimestampMicros::now()).await.unwrap();
    assert_eq!(
        incidents.inner.lock().unwrap().len(),
        0,
        "value within baseline must not open an incident"
    );
}

// ───────────────────── incident grouping (semantic groups) ─────────────────────

/// 记录 upsert_for_incident 的 (org, scope_id, fingerprint)，用于断言分组 key。
#[derive(Default)]
struct RecordingIncidentGroups {
    upserts: StdMutex<Vec<(String, String, String)>>,
}
#[async_trait]
impl IncidentGroupRepository for RecordingIncidentGroups {
    async fn upsert_for_incident(
        &self,
        org_id: &Id,
        alert_rule_id: &Id,
        fingerprint: &str,
        at: TimestampMicros,
        _window_secs: i64,
    ) -> Result<IncidentGroup> {
        self.upserts.lock().unwrap().push((
            org_id.0.clone(),
            alert_rule_id.0.clone(),
            fingerprint.to_string(),
        ));
        Ok(IncidentGroup {
            id: Id::from_string("ig-1"),
            org_id: org_id.clone(),
            alert_rule_id: alert_rule_id.clone(),
            fingerprint: fingerprint.to_string(),
            state: IncidentGroupState::Open,
            incident_count: 1,
            first_at: at,
            last_at: at,
            acked_by: None,
            acked_at: None,
            resolved_at: None,
        })
    }
    async fn get(&self, _: &Id, _: &Id) -> Result<IncidentGroup> {
        Err(molesignal::shared::Error::not_found("group"))
    }
    async fn list(
        &self,
        _: &Id,
        _: Option<IncidentGroupState>,
        _: i64,
    ) -> Result<Vec<IncidentGroup>> {
        Ok(vec![])
    }
    async fn ack(&self, _: &Id, _: &Id, _: &Id, _: TimestampMicros) -> Result<()> {
        Ok(())
    }
    async fn resolve(&self, _: &Id, _: &Id, _: TimestampMicros) -> Result<()> {
        Ok(())
    }
}

struct StaticSemanticGroups {
    groups: Vec<SemanticGroup>,
}
#[async_trait]
impl SemanticGroupRepository for StaticSemanticGroups {
    async fn list_enabled(&self, org_id: &Id) -> Result<Vec<SemanticGroup>> {
        Ok(self
            .groups
            .iter()
            .filter(|g| g.enabled && &g.org_id == org_id)
            .cloned()
            .collect())
    }
    async fn create(&self, g: SemanticGroup) -> Result<SemanticGroup> {
        Ok(g)
    }
    async fn update(&self, g: SemanticGroup) -> Result<SemanticGroup> {
        Ok(g)
    }
    async fn get(&self, _: &Id) -> Result<SemanticGroup> {
        Err(molesignal::shared::Error::not_found("group"))
    }
    async fn list(&self, _: &Id) -> Result<Vec<SemanticGroup>> {
        Ok(self.groups.clone())
    }
    async fn delete(&self, _: &Id) -> Result<()> {
        Ok(())
    }
}

fn grouping_rule() -> AlertRule {
    let mut r = sample_rule();
    r.trigger.for_periods = 1;
    r.labels = BTreeMap::from([
        ("severity".to_string(), "critical".to_string()),
        ("service".to_string(), "api".to_string()),
    ]);
    r
}

fn crit_group() -> SemanticGroup {
    SemanticGroup {
        id: Id::from_string("sg-crit"),
        org_id: Id::from_string("org-a"),
        name: "crit-by-service".into(),
        enabled: true,
        matchers: vec![LabelMatcher {
            label: "severity".into(),
            op: MatchOp::Eq,
            value: "critical".into(),
        }],
        group_by: vec!["service".into()],
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

#[tokio::test]
async fn evaluator_assigns_incident_to_matching_semantic_group() {
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![grouping_rule()]),
    });
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let eval_state: Arc<InMemEvalState> = Arc::new(InMemEvalState::default());
    let engine = Arc::new(ScriptedEngine {
        values: StdMutex::new(vec![15.0]), // > threshold 10 → fires this tick
    });
    let ig = Arc::new(RecordingIncidentGroups::default());
    let sg = Arc::new(StaticSemanticGroups {
        groups: vec![crit_group()],
    });
    let evaluator = RuleEvaluator::new(
        rules,
        incidents.clone() as Arc<dyn IncidentRepository>,
        eval_state,
        engine,
        10,
    )
    .with_grouping(
        ig.clone() as Arc<dyn IncidentGroupRepository>,
        sg.clone() as Arc<dyn SemanticGroupRepository>,
    );

    evaluator.tick(TimestampMicros::now()).await.unwrap();
    assert_eq!(incidents.inner.lock().unwrap().len(), 1);
    let ups = ig.upserts.lock().unwrap();
    assert_eq!(ups.len(), 1, "one incident => one grouping upsert");
    assert_eq!(ups[0].0, "org-a");
    assert_eq!(ups[0].1, "sg-crit", "scope = matched semantic group id");
    assert_eq!(
        ups[0].2, "service=api",
        "fingerprint = group_by-derived key"
    );
}

#[tokio::test]
async fn evaluator_groups_by_rule_when_no_semantic_group_matches() {
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![grouping_rule()]),
    });
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let eval_state: Arc<InMemEvalState> = Arc::new(InMemEvalState::default());
    let engine = Arc::new(ScriptedEngine {
        values: StdMutex::new(vec![15.0]),
    });
    let ig = Arc::new(RecordingIncidentGroups::default());
    // matcher 要 severity=info，但 incident 是 critical → 不命中 → 回退到逐规则分组。
    let mut g = crit_group();
    g.matchers[0].value = "info".into();
    let sg = Arc::new(StaticSemanticGroups { groups: vec![g] });
    let evaluator = RuleEvaluator::new(
        rules,
        incidents.clone() as Arc<dyn IncidentRepository>,
        eval_state,
        engine,
        10,
    )
    .with_grouping(
        ig.clone() as Arc<dyn IncidentGroupRepository>,
        sg.clone() as Arc<dyn SemanticGroupRepository>,
    );

    evaluator.tick(TimestampMicros::now()).await.unwrap();
    let ups = ig.upserts.lock().unwrap();
    assert_eq!(ups.len(), 1);
    assert_eq!(
        ups[0].1, "rule-1",
        "no match => scope falls back to rule id"
    );
}

// ---- AlertingService / 评估状态 / 静默 / PromQL 路由 ----

struct InMemPolicies {
    inner: StdMutex<Vec<EscalationPolicy>>,
}
#[async_trait]
impl EscalationPolicyRepository for InMemPolicies {
    async fn create(&self, policy: EscalationPolicy) -> Result<EscalationPolicy> {
        self.inner.lock().unwrap().push(policy.clone());
        Ok(policy)
    }
    async fn update(&self, policy: EscalationPolicy) -> Result<EscalationPolicy> {
        Ok(policy)
    }
    async fn get(&self, id: &Id) -> Result<EscalationPolicy> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .find(|p| &p.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("policy"))
    }
    async fn list(&self, org_id: &Id) -> Result<Vec<EscalationPolicy>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|p| &p.org_id == org_id)
            .cloned()
            .collect())
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

struct NoSchedules;
#[async_trait]
impl ScheduleRepository for NoSchedules {
    async fn create(&self, schedule: Schedule) -> Result<Schedule> {
        Ok(schedule)
    }
    async fn update(&self, schedule: Schedule) -> Result<Schedule> {
        Ok(schedule)
    }
    async fn get(&self, _id: &Id) -> Result<Schedule> {
        Err(Error::not_found("schedule"))
    }
    async fn list(&self, _org_id: &Id) -> Result<Vec<Schedule>> {
        Ok(Vec::new())
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

fn policy(id: &str, org: &str, name: &str) -> EscalationPolicy {
    EscalationPolicy {
        id: Id::from_string(id),
        org_id: Id::from_string(org),
        name: name.into(),
        steps: Vec::new(),
        repeat: false,
        max_loops: 1,
        ack_retrigger_secs: None,
        urgency_rule: Default::default(),
        escalate_low_urgency: false,
    }
}

fn alerting_service(
    rules: Arc<InMemRules>,
    incidents: Arc<InMemIncidents>,
    policies: Vec<EscalationPolicy>,
) -> AlertingService {
    AlertingService::new(
        rules,
        incidents,
        Arc::new(NoSchedules),
        Arc::new(InMemPolicies {
            inner: StdMutex::new(policies),
        }),
    )
}

fn open_incident_fixture() -> Incident {
    Incident {
        id: Id::from_string("incident-1"),
        org_id: Id::from_string("org-a"),
        rule_id: Id::from_string("rule-1"),
        escalation_policy_id: Id::from_string("pol-1"),
        status: IncidentStatus::Open,
        severity: Severity::Warning,
        summary: "high latency".into(),
        fingerprint: "fp".into(),
        current_step: 0,
        current_loop: 0,
        current_step_started_at: TimestampMicros(0),
        assignees: Vec::new(),
        labels: BTreeMap::new(),
        annotations: BTreeMap::new(),
        trace_ids: Vec::new(),
        host_ids: Vec::new(),
        affected_services: Vec::new(),
        triggering_query: None,
        created_at: TimestampMicros(0),
        acknowledged_at: None,
        acknowledged_by: None,
        resolved_at: None,
        resolved_by: None,
        team_id: None,
        routing: None,
        handling: Default::default(),
        revision: 0,
    }
}

fn empty_rules() -> Arc<InMemRules> {
    Arc::new(InMemRules {
        inner: StdMutex::new(Vec::new()),
    })
}

#[tokio::test]
async fn acknowledge_and_resolve_follow_the_incident_state_machine() {
    let incidents = Arc::new(InMemIncidents::default());
    let open = open_incident_fixture();
    incidents.create(open.clone()).await.unwrap();
    let service = alerting_service(empty_rules(), incidents.clone(), Vec::new());
    let user = |name: &str| Id::from_string(name);

    let acked = service
        .acknowledge(&open.id, user("u1"), TimestampMicros(10), ActionVia::Web)
        .await
        .unwrap();
    assert_eq!(acked.status, IncidentStatus::Acknowledged);
    assert_eq!(acked.acknowledged_by, Some(user("u1")));
    assert_eq!(acked.assignees, vec![user("u1")]);

    // 重复确认幂等：保留最初的确认人与时间。
    let again = service
        .acknowledge(&open.id, user("u2"), TimestampMicros(20), ActionVia::Web)
        .await
        .unwrap();
    assert_eq!(again.acknowledged_by, Some(user("u1")));
    assert_eq!(again.acknowledged_at.map(|t| t.0), Some(10));

    let resolved = service
        .resolve(&open.id, user("u3"), TimestampMicros(30), ActionVia::Web)
        .await
        .unwrap();
    assert_eq!(resolved.status, IncidentStatus::Resolved);
    assert_eq!(resolved.resolved_by, Some(user("u3")));

    // 重复解决幂等：不覆盖解决人与时间。
    let again = service
        .resolve(&open.id, user("u4"), TimestampMicros(40), ActionVia::Web)
        .await
        .unwrap();
    assert_eq!(again.resolved_by, Some(user("u3")));
    assert_eq!(again.resolved_at.map(|t| t.0), Some(30));

    // 已解决的事件不能再被确认，状态保持 resolved。
    let error = service
        .acknowledge(&open.id, user("u5"), TimestampMicros(50), ActionVia::Web)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Conflict(_)), "{error}");
    let stored = incidents.get(&open.id).await.unwrap();
    assert_eq!(stored.status, IncidentStatus::Resolved);
    assert_eq!(stored.acknowledged_by, Some(user("u1")));
}

#[tokio::test]
async fn open_incident_can_be_resolved_without_acknowledgement() {
    let incidents = Arc::new(InMemIncidents::default());
    let open = open_incident_fixture();
    incidents.create(open.clone()).await.unwrap();
    let service = alerting_service(empty_rules(), incidents, Vec::new());

    let resolved = service
        .resolve(
            &open.id,
            Id::from_string("u1"),
            TimestampMicros(5),
            ActionVia::Web,
        )
        .await
        .unwrap();

    assert_eq!(resolved.status, IncidentStatus::Resolved);
    assert!(resolved.acknowledged_by.is_none());
}

#[tokio::test]
async fn rule_policy_reference_must_exist_unless_it_is_the_default_alias() {
    let rules = empty_rules();
    let service = alerting_service(
        rules.clone(),
        Arc::new(InMemIncidents::default()),
        vec![
            policy("pol-1", "org-a", "primary"),
            policy("other-org", "org-b", "x"),
        ],
    );
    let mut rule = sample_rule();

    service
        .create_rule(rule.clone())
        .await
        .expect("existing policy");

    rule.id = Id::from_string("rule-2");
    rule.escalation_policy_id = Id::from_string(DEFAULT_POLICY_ID);
    service
        .create_rule(rule.clone())
        .await
        .expect("default alias");

    for unknown in ["nope", "other-org"] {
        rule.id = Id::from_string("rule-3");
        rule.escalation_policy_id = Id::from_string(unknown);
        let error = service.create_rule(rule.clone()).await.unwrap_err();
        assert!(
            matches!(error, Error::InvalidArgument(_)),
            "{unknown}: {error}"
        );
    }

    // 只校验被改动的引用：未改动的引用即便策略已不存在，也不阻止改其它字段。
    let mut stored = rules.get(&Id::from_string("rule-1")).await.unwrap();
    stored.name = "renamed".into();
    let service = alerting_service(
        rules.clone(),
        Arc::new(InMemIncidents::default()),
        Vec::new(),
    );
    service
        .update_rule(stored.clone())
        .await
        .expect("unchanged reference");
    stored.escalation_policy_id = Id::from_string("nope");
    let error = service.update_rule(stored).await.unwrap_err();
    assert!(matches!(error, Error::InvalidArgument(_)), "{error}");
}

fn evaluator_with(
    rules: Arc<InMemRules>,
    incidents: Arc<InMemIncidents>,
    engine: Arc<dyn QueryEngine>,
) -> (RuleEvaluator, Arc<InMemEvalState>) {
    let eval_state = Arc::new(InMemEvalState::default());
    let evaluator = RuleEvaluator::new(rules, incidents, eval_state.clone(), engine, 10);
    (evaluator, eval_state)
}

fn scripted(values: Vec<f64>) -> Arc<ScriptedEngine> {
    // 引擎从栈顶取值，脚本按相反顺序压入。
    Arc::new(ScriptedEngine {
        values: StdMutex::new(values.into_iter().rev().collect()),
    })
}

#[tokio::test]
async fn evaluator_records_last_eval_and_state_on_the_rule() {
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![sample_rule()]),
    });
    let (evaluator, _) = evaluator_with(
        rules.clone(),
        Arc::new(InMemIncidents::default()),
        scripted(vec![15.0, 15.0, 5.0]),
    );
    let rule_id = Id::from_string("rule-1");
    let at = |n: i64| TimestampMicros(1_000_000_000 + n * 60_000_000);

    evaluator.tick(at(0)).await.unwrap();
    let rule = rules.get(&rule_id).await.unwrap();
    assert_eq!(rule.last_eval_at, Some(at(0)));
    assert_eq!(rule.last_state, RuleState::Pending(1));

    evaluator.tick(at(1)).await.unwrap();
    let rule = rules.get(&rule_id).await.unwrap();
    assert_eq!(rule.last_eval_at, Some(at(1)));
    assert_eq!(rule.last_state, RuleState::Firing);

    evaluator.tick(at(2)).await.unwrap();
    let rule = rules.get(&rule_id).await.unwrap();
    assert_eq!(rule.last_eval_at, Some(at(2)));
    assert_eq!(rule.last_state, RuleState::Healthy);
}

#[tokio::test]
async fn failed_query_leaves_the_rule_evaluation_untouched() {
    struct Failing;
    #[async_trait]
    impl QueryEngine for Failing {
        async fn execute(&self, _req: QueryRequest) -> Result<QueryResult> {
            Err(Error::invalid("query could not be planned"))
        }
    }
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![sample_rule()]),
    });
    let (evaluator, _) = evaluator_with(
        rules.clone(),
        Arc::new(InMemIncidents::default()),
        Arc::new(Failing),
    );

    evaluator.tick(TimestampMicros(1)).await.unwrap();

    let rule = rules.get(&Id::from_string("rule-1")).await.unwrap();
    assert!(rule.last_eval_at.is_none());
    assert_eq!(rule.last_state, RuleState::default());
}

#[tokio::test]
async fn silence_secs_suppresses_a_new_incident_inside_the_cool_down() {
    let mut rule = sample_rule();
    rule.trigger.for_periods = 1;
    rule.trigger.silence_secs = 300;
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![rule]),
    });
    let incidents = Arc::new(InMemIncidents::default());
    // 命中 → 恢复 → 再命中（静默期内）→ 再恢复 → 再命中（静默期外）
    let (evaluator, _) = evaluator_with(
        rules,
        incidents.clone(),
        scripted(vec![15.0, 5.0, 15.0, 5.0, 15.0]),
    );
    let base = 1_000_000_000_i64;
    let at = |secs: i64| TimestampMicros(base + secs * 1_000_000);

    evaluator.tick(at(0)).await.unwrap();
    assert_eq!(incidents.inner.lock().unwrap().len(), 1);

    evaluator.tick(at(60)).await.unwrap();
    assert_eq!(
        incidents.inner.lock().unwrap()[0].status,
        IncidentStatus::Resolved
    );

    evaluator.tick(at(120)).await.unwrap();
    assert_eq!(
        incidents.inner.lock().unwrap().len(),
        1,
        "re-fire inside silence_secs must not open a new incident"
    );

    evaluator.tick(at(180)).await.unwrap();
    evaluator.tick(at(400)).await.unwrap();
    let all = incidents.inner.lock().unwrap();
    assert_eq!(
        all.len(),
        2,
        "re-fire after silence_secs opens a new incident"
    );
    assert_eq!(all[1].status, IncidentStatus::Open);
}

#[tokio::test]
async fn zero_silence_secs_reopens_immediately() {
    let mut rule = sample_rule();
    rule.trigger.for_periods = 1;
    rule.trigger.silence_secs = 0;
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![rule]),
    });
    let incidents = Arc::new(InMemIncidents::default());
    let (evaluator, _) = evaluator_with(rules, incidents.clone(), scripted(vec![15.0, 5.0, 15.0]));

    for step in 0..3 {
        evaluator
            .tick(TimestampMicros(1_000_000_000 + step * 60_000_000))
            .await
            .unwrap();
    }

    assert_eq!(incidents.inner.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn acknowledged_incident_is_neither_duplicated_nor_auto_resolved() {
    let mut rule = sample_rule();
    rule.trigger.for_periods = 1;
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![rule]),
    });
    let incidents = Arc::new(InMemIncidents::default());
    let (evaluator, _) = evaluator_with(
        rules.clone(),
        incidents.clone(),
        scripted(vec![15.0, 15.0, 5.0]),
    );
    let service = alerting_service(rules, incidents.clone(), Vec::new());
    let at = |n: i64| TimestampMicros(1_000_000_000 + n * 60_000_000);

    evaluator.tick(at(0)).await.unwrap();
    let id = incidents.inner.lock().unwrap()[0].id.clone();
    service
        .acknowledge(&id, Id::from_string("u1"), at(0), ActionVia::Web)
        .await
        .unwrap();

    // 仍在触发：同一指纹已有活跃事件，不再开新的。
    evaluator.tick(at(1)).await.unwrap();
    assert_eq!(incidents.inner.lock().unwrap().len(), 1);

    // 不再命中：已确认的事件保持 acknowledged，等人来解决。
    evaluator.tick(at(2)).await.unwrap();
    let all = incidents.inner.lock().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].status, IncidentStatus::Acknowledged);
}

#[tokio::test]
async fn promql_rules_are_evaluated_by_the_promql_engine() {
    struct ScriptedPromql(f64);
    #[async_trait]
    impl PromqlEngine for ScriptedPromql {
        async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
            assert_eq!(req.language, QueryLanguage::Promql);
            Ok(QueryResult {
                columns: vec!["value".into()],
                rows: vec![vec![serde_json::json!(self.0)]],
                scanned_rows: 1,
                took_ms: 0,
                federation: None,
            })
        }
    }

    let mut rule = sample_rule();
    rule.query.language = QueryLanguage::Promql;
    rule.query.statement = "queue_depth".into();
    rule.trigger.for_periods = 1;
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![rule]),
    });
    let incidents = Arc::new(InMemIncidents::default());
    // SQL 引擎对任何请求都返回 0，若 PromQL 规则被误送到这里就不会触发。
    let engine = LanguageRoutedEngine::new(scripted(Vec::new()), Arc::new(ScriptedPromql(42.0)));
    let (evaluator, _) = evaluator_with(rules, incidents.clone(), Arc::new(engine));

    evaluator
        .tick(TimestampMicros(1_000_000_000))
        .await
        .unwrap();

    let all = incidents.inner.lock().unwrap();
    assert_eq!(all.len(), 1);
    let query = all[0].triggering_query.as_ref().expect("triggering query");
    assert_eq!(query.language, QueryLanguage::Promql);
}

// ───────────────── 多个 alert 节点分担评估 ─────────────────

/// 固定成员表：节点只看到表里的 alert 节点。
struct AlertMembers(Vec<PeerInfo>);

#[async_trait]
impl ClusterRegistry for AlertMembers {
    async fn list_role(&self, _role: PeerRole) -> Vec<PeerInfo> {
        self.0.clone()
    }
}

fn alert_members(node_ids: &[&str]) -> Arc<AlertMembers> {
    Arc::new(AlertMembers(
        node_ids
            .iter()
            .map(|node_id| PeerInfo {
                node_id: (*node_id).into(),
                advertise_addr: format!("{node_id}:5082"),
                roles: vec![PeerRole::Alert],
            })
            .collect(),
    ))
}

/// 不需要预热的归属：测试里节点一加入就拥有自己的 key。
fn ownership(members: &Arc<AlertMembers>, node_id: &str) -> Arc<WorkOwnership> {
    WorkOwnership::new(members.clone(), node_id, PeerRole::Alert, Duration::ZERO)
}

/// 总返回同一个值、并记录被调用次数的引擎。
struct ConstantEngine {
    value: f64,
    calls: AtomicUsize,
}

impl ConstantEngine {
    fn new(value: f64) -> Arc<Self> {
        Arc::new(Self {
            value,
            calls: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl QueryEngine for ConstantEngine {
    async fn execute(&self, _req: QueryRequest) -> Result<QueryResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(QueryResult {
            columns: vec!["v".into()],
            rows: vec![vec![serde_json::json!(self.value)]],
            scanned_rows: 1,
            took_ms: 0,
            federation: None,
        })
    }
}

fn rules_named(count: usize) -> Arc<InMemRules> {
    let rules = (0..count)
        .map(|index| {
            let mut rule = sample_rule();
            rule.id = Id::from_string(format!("rule-{index}"));
            rule
        })
        .collect();
    Arc::new(InMemRules {
        inner: StdMutex::new(rules),
    })
}

async fn matches_of(eval_state: &InMemEvalState, rule_id: &str) -> u32 {
    eval_state
        .get(&Id::from_string(rule_id), "")
        .await
        .unwrap()
        .map_or(0, |state| state.consecutive_matches)
}

#[tokio::test]
async fn alert_nodes_split_the_rules_between_them() {
    let rules = rules_named(30);
    let incidents = Arc::new(InMemIncidents::default());
    let eval_state = Arc::new(InMemEvalState::default());
    let members = alert_members(&["alert-a", "alert-b"]);
    let engines = [ConstantEngine::new(15.0), ConstantEngine::new(15.0)];
    let evaluators: Vec<RuleEvaluator> = ["alert-a", "alert-b"]
        .iter()
        .zip(&engines)
        .map(|(node_id, engine)| {
            RuleEvaluator::new(
                rules.clone(),
                incidents.clone(),
                eval_state.clone(),
                engine.clone(),
                10,
            )
            .with_ownership(ownership(&members, node_id))
        })
        .collect();

    for evaluator in &evaluators {
        evaluator
            .tick(TimestampMicros(1_000_000_000))
            .await
            .unwrap();
    }

    // 每条规则只被它的 owner 评估一次；两个节点都分到了规则。
    let (on_a, on_b) = (engines[0].calls(), engines[1].calls());
    assert_eq!(on_a + on_b, 30, "{on_a} + {on_b}");
    assert!(on_a > 0 && on_b > 0, "{on_a} + {on_b}");
    for index in 0..30 {
        assert_eq!(matches_of(&eval_state, &format!("rule-{index}")).await, 1);
    }
}

#[tokio::test]
async fn a_departed_node_hands_its_rules_to_the_survivor() {
    let rules = rules_named(30);
    let eval_state = Arc::new(InMemEvalState::default());
    let engine = ConstantEngine::new(15.0);
    // 只剩 alert-a 一个成员：它拥有全部规则，包括原先归 alert-b 的那一半。
    let survivor = RuleEvaluator::new(
        rules,
        Arc::new(InMemIncidents::default()),
        eval_state,
        engine.clone(),
        10,
    )
    .with_ownership(ownership(&alert_members(&["alert-a"]), "alert-a"));

    survivor.tick(TimestampMicros(1_000_000_000)).await.unwrap();

    assert_eq!(engine.calls(), 30);
}

/// 两个节点各自只看到自己，都认为自己拥有规则：成员变化窗口里的双跑。
fn overlapping_evaluators(
    rules: &Arc<InMemRules>,
    incidents: &Arc<InMemIncidents>,
    eval_state: &Arc<InMemEvalState>,
    engine: &Arc<ConstantEngine>,
    cycle_guard: Option<Duration>,
) -> [RuleEvaluator; 2] {
    ["alert-a", "alert-b"].map(|node_id| {
        let evaluator = RuleEvaluator::new(
            rules.clone(),
            incidents.clone(),
            eval_state.clone(),
            engine.clone(),
            10,
        )
        .with_ownership(ownership(&alert_members(&[node_id]), node_id));
        match cycle_guard {
            Some(gap) => evaluator.with_cycle_guard(gap),
            None => evaluator,
        }
    })
}

#[tokio::test]
async fn an_overlapping_cycle_is_counted_once_with_the_cycle_guard() {
    let rules = rules_named(1);
    let incidents = Arc::new(InMemIncidents::default());
    let eval_state = Arc::new(InMemEvalState::default());
    let engine = ConstantEngine::new(15.0);
    let [a, b] = overlapping_evaluators(
        &rules,
        &incidents,
        &eval_state,
        &engine,
        Some(Duration::from_secs(15)),
    );
    let t0 = 1_000_000_000;
    let at = |seconds: i64| TimestampMicros(t0 + seconds * 1_000_000);

    a.tick(at(0)).await.unwrap();
    // 5 秒后另一个节点也评估同一条规则：属于同一个周期，不计数、也不再执行查询。
    b.tick(at(5)).await.unwrap();
    assert_eq!(matches_of(&eval_state, "rule-0").await, 1);
    assert_eq!(engine.calls(), 1);
    assert!(incidents.inner.lock().unwrap().is_empty());

    // 下一个周期（相隔一个评估间隔）正常计数：for_periods = 2 达成，开事件。
    b.tick(at(30)).await.unwrap();
    assert_eq!(matches_of(&eval_state, "rule-0").await, 2);
    assert_eq!(incidents.inner.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn without_the_cycle_guard_an_overlapping_cycle_is_counted_twice() {
    let rules = rules_named(1);
    let incidents = Arc::new(InMemIncidents::default());
    let eval_state = Arc::new(InMemEvalState::default());
    let engine = ConstantEngine::new(15.0);
    let [a, b] = overlapping_evaluators(&rules, &incidents, &eval_state, &engine, None);

    a.tick(TimestampMicros(1_000_000_000)).await.unwrap();
    b.tick(TimestampMicros(1_005_000_000)).await.unwrap();

    // 守卫要挡住的正是这种情况：同一个周期被算成两次，for_periods 被提前耗尽。
    assert_eq!(matches_of(&eval_state, "rule-0").await, 2);
    assert_eq!(incidents.inner.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn multi_tier_rules_are_counted_once_per_cycle_too() {
    let mut rule = sample_rule();
    rule.thresholds = vec![SeverityThreshold {
        severity: Severity::Warning,
        operator: ComparisonOp::Gt,
        threshold: 10.0,
        for_periods: 3,
    }];
    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![rule]),
    });
    let incidents = Arc::new(InMemIncidents::default());
    let eval_state = Arc::new(InMemEvalState::default());
    let engine = ConstantEngine::new(15.0);
    let [a, b] = overlapping_evaluators(
        &rules,
        &incidents,
        &eval_state,
        &engine,
        Some(Duration::from_secs(15)),
    );

    a.tick(TimestampMicros(1_000_000_000)).await.unwrap();
    b.tick(TimestampMicros(1_005_000_000)).await.unwrap();

    let state = eval_state
        .get(&Id::from_string("rule-1"), "")
        .await
        .unwrap()
        .expect("state row");
    assert_eq!(state.consecutive_matches, 1);
    assert_eq!(state.severity_streaks.values().sum::<u32>(), 1);
}

// ───────────────────────── split by columns (query.group_by) ─────────────────────────

/// 每次 execute 依次返回一份按主机分组的结果，并记下请求的行数上限。
struct GroupedEngine {
    rounds: StdMutex<Vec<Vec<(&'static str, f64)>>>,
    limits: StdMutex<Vec<Option<usize>>>,
}
#[async_trait]
impl QueryEngine for GroupedEngine {
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        self.limits.lock().unwrap().push(req.limit);
        let rows = self.rounds.lock().unwrap().remove(0);
        Ok(QueryResult {
            columns: vec!["host".into(), "latency".into()],
            rows: rows
                .into_iter()
                .map(|(host, value)| vec![serde_json::json!(host), serde_json::json!(value)])
                .collect(),
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        })
    }
}

#[tokio::test]
async fn a_rule_split_by_host_opens_and_resolves_one_incident_per_host() {
    let mut rule = sample_rule();
    rule.query.statement = "SELECT host, max(latency) FROM logs GROUP BY host".into();
    rule.query.group_by = vec!["host".into()];

    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![rule]),
    });
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let eval_state: Arc<InMemEvalState> = Arc::new(InMemEvalState::default());
    let engine = Arc::new(GroupedEngine {
        rounds: StdMutex::new(vec![
            vec![("web-1", 15.0), ("web-2", 20.0)],
            vec![("web-1", 15.0), ("web-3", 30.0)],
            vec![("web-2", 20.0), ("web-3", 30.0)],
            vec![],
        ]),
        limits: StdMutex::new(Vec::new()),
    });
    let evaluator = RuleEvaluator::new(
        rules.clone() as Arc<dyn AlertRuleRepository>,
        incidents.clone() as Arc<dyn IncidentRepository>,
        eval_state.clone() as Arc<dyn AlertRuleEvalStateRepository>,
        engine.clone(),
        10,
    );

    let open_hosts = || {
        let mut hosts = incidents
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|i| i.status == IncidentStatus::Open)
            .map(|i| i.labels["host"].clone())
            .collect::<Vec<_>>();
        hosts.sort();
        hosts
    };
    let rule_state = || rules.inner.lock().unwrap()[0].last_state;

    let minute = 60_000_000;
    let start = TimestampMicros::now();

    // 1：两台主机各命中一轮，for_periods=2 还不够。
    evaluator.tick(start).await.unwrap();
    assert!(open_hosts().is_empty());
    assert_eq!(rule_state(), RuleState::Pending(1));

    // 2：web-1 连续两轮 → 开事件；web-2 不在结果里，连续命中从头算；web-3 第一轮。
    evaluator
        .tick(TimestampMicros(start.0 + minute))
        .await
        .unwrap();
    assert_eq!(open_hosts(), ["web-1"]);
    assert_eq!(rule_state(), RuleState::Firing);
    {
        let g = incidents.inner.lock().unwrap();
        assert_eq!(g[0].summary, "high latency · web-1");
    }

    // 3：web-1 没了 → 它的事件解决；web-3 连续两轮 → 开事件；web-2 只算第一轮。
    evaluator
        .tick(TimestampMicros(start.0 + 2 * minute))
        .await
        .unwrap();
    assert_eq!(open_hosts(), ["web-3"]);
    assert_eq!(incidents.inner.lock().unwrap().len(), 2);

    // 4：结果为空 → 全部解决，各组的状态清掉，规则本身那一行留着。
    evaluator
        .tick(TimestampMicros(start.0 + 3 * minute))
        .await
        .unwrap();
    assert!(open_hosts().is_empty());
    assert_eq!(rule_state(), RuleState::Healthy);
    assert!(
        eval_state
            .inner
            .lock()
            .unwrap()
            .keys()
            .all(|(_, group)| group.is_empty())
    );

    // SQL 多取一行，看得出组数有没有超过上限。
    let limits = engine.limits.lock().unwrap().clone();
    assert_eq!(limits, vec![Some(MAX_ALERT_GROUPS + 1); 4]);
}

// ───────────────────────── no data (trigger.no_data) ─────────────────────────

/// 每次 execute 依次返回一份结果，`None` 是一行都没有。
struct GapEngine {
    rounds: StdMutex<Vec<Option<f64>>>,
}
#[async_trait]
impl QueryEngine for GapEngine {
    async fn execute(&self, _req: QueryRequest) -> Result<QueryResult> {
        let value = self.rounds.lock().unwrap().remove(0);
        Ok(QueryResult {
            columns: vec!["v".into()],
            rows: value
                .map(|value| vec![serde_json::json!(value)])
                .into_iter()
                .collect(),
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        })
    }
}

#[tokio::test]
async fn a_rule_without_data_keeps_its_incident_and_opens_a_no_data_one_until_data_returns() {
    let mut rule = sample_rule();
    rule.trigger.no_data = Some(NoDataCondition {
        action: NoDataAction::Alert,
        for_periods: 2,
    });

    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![rule]),
    });
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let eval_state: Arc<InMemEvalState> = Arc::new(InMemEvalState::default());
    let engine = Arc::new(GapEngine {
        rounds: StdMutex::new(vec![Some(15.0), Some(15.0), None, None, None, Some(5.0)]),
    });
    let evaluator = RuleEvaluator::new(
        rules.clone() as Arc<dyn AlertRuleRepository>,
        incidents.clone() as Arc<dyn IncidentRepository>,
        eval_state as Arc<dyn AlertRuleEvalStateRepository>,
        engine,
        10,
    );

    let statuses = || {
        incidents
            .inner
            .lock()
            .unwrap()
            .iter()
            .map(|i| (i.labels.contains_key("alert.condition"), i.status))
            .collect::<Vec<_>>()
    };
    let start = TimestampMicros::now();
    let tick = |round: i64| TimestampMicros(start.0 + round * 60_000_000);

    evaluator.tick(tick(0)).await.unwrap();
    evaluator.tick(tick(1)).await.unwrap();
    assert_eq!(statuses(), [(false, IncidentStatus::Open)]);

    // No value: the open incident is kept, and the rule says it has no data.
    evaluator.tick(tick(2)).await.unwrap();
    assert_eq!(statuses(), [(false, IncidentStatus::Open)]);
    assert_eq!(rules.inner.lock().unwrap()[0].last_state, RuleState::NoData);

    // The second round without a value opens the no-data incident, once.
    evaluator.tick(tick(3)).await.unwrap();
    evaluator.tick(tick(4)).await.unwrap();
    assert_eq!(
        statuses(),
        [(false, IncidentStatus::Open), (true, IncidentStatus::Open)]
    );
    {
        let g = incidents.inner.lock().unwrap();
        assert_eq!(g[1].summary, "high latency");
        assert_eq!(g[1].labels["alert.condition"], "no_data");
    }

    // A healthy value resolves both.
    evaluator.tick(tick(5)).await.unwrap();
    assert_eq!(
        statuses(),
        [
            (false, IncidentStatus::Resolved),
            (true, IncidentStatus::Resolved)
        ]
    );
}

#[tokio::test]
async fn a_host_that_stops_reporting_opens_its_own_no_data_incident_until_someone_resolves_it() {
    let mut rule = sample_rule();
    rule.query.group_by = vec!["host".into()];
    rule.trigger.for_periods = 1;
    rule.trigger.no_data = Some(NoDataCondition {
        action: NoDataAction::Alert,
        for_periods: 2,
    });

    let rules = Arc::new(InMemRules {
        inner: StdMutex::new(vec![rule]),
    });
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let eval_state: Arc<InMemEvalState> = Arc::new(InMemEvalState::default());
    let engine = Arc::new(GroupedEngine {
        rounds: StdMutex::new(vec![
            vec![("web-1", 1.0), ("web-2", 1.0)],
            vec![("web-2", 1.0)],
            vec![("web-2", 1.0)],
            vec![("web-2", 1.0)],
            vec![("web-2", 1.0)],
            vec![("web-2", 1.0)],
        ]),
        limits: StdMutex::new(Vec::new()),
    });
    let evaluator = RuleEvaluator::new(
        rules.clone() as Arc<dyn AlertRuleRepository>,
        incidents.clone() as Arc<dyn IncidentRepository>,
        eval_state.clone() as Arc<dyn AlertRuleEvalStateRepository>,
        engine,
        10,
    );

    let groups_known = || {
        eval_state
            .inner
            .lock()
            .unwrap()
            .keys()
            .filter(|(_, group)| !group.is_empty())
            .count()
    };
    let start = TimestampMicros::now();
    let tick = |round: i64| TimestampMicros(start.0 + round * 60_000_000);

    evaluator.tick(tick(0)).await.unwrap();
    evaluator.tick(tick(1)).await.unwrap();
    assert!(incidents.inner.lock().unwrap().is_empty());
    assert_eq!(rules.inner.lock().unwrap()[0].last_state, RuleState::NoData);

    // web-1 has been gone for two rounds: its own no-data incident opens, once.
    evaluator.tick(tick(2)).await.unwrap();
    evaluator.tick(tick(3)).await.unwrap();
    {
        let g = incidents.inner.lock().unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].summary, "high latency · web-1");
        assert_eq!(g[0].labels["host"], "web-1");
        assert_eq!(g[0].labels["alert.condition"], "no_data");
        assert_eq!(g[0].status, IncidentStatus::Open);
    }

    // Someone resolves it: the host is gone for good, so it is not reopened and is forgotten.
    {
        let mut g = incidents.inner.lock().unwrap();
        g[0].status = IncidentStatus::Resolved;
        g[0].resolved_at = Some(tick(3));
        g[0].resolved_by = Some(Id::from_string("user-1"));
    }
    assert_eq!(groups_known(), 2);

    evaluator.tick(tick(4)).await.unwrap();
    evaluator.tick(tick(5)).await.unwrap();
    assert_eq!(incidents.inner.lock().unwrap().len(), 1);
    assert_eq!(groups_known(), 1);
    assert_eq!(
        rules.inner.lock().unwrap()[0].last_state,
        RuleState::Healthy
    );
}

// ───────────────────────── synthetic checks open incidents ─────────────────────────

fn synthetic_monitor() -> SyntheticMonitor {
    SyntheticMonitor {
        id: Id::from_string("monitor-1"),
        organization_id: Id::from_string("org-a"),
        name: "Checkout API".into(),
        description: String::new(),
        kind: MonitorKind::Http,
        lifecycle: MonitorLifecycle::Active,
        state: MonitorState::Healthy,
        team_id: Some(Id::from_string("team-1")),
        tags: vec!["production".into(), "checkout".into()],
        active_revision_id: Some(Id::from_string("rev-1")),
        draft_revision_id: None,
        next_due_at: None,
        created_by: Id::from_string("user-1"),
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
        archived_at: None,
    }
}

fn synthetic_revision(escalation_policy_id: Option<&str>) -> MonitorRevision {
    MonitorRevision {
        id: Id::from_string("rev-1"),
        organization_id: Id::from_string("org-a"),
        monitor_id: Id::from_string("monitor-1"),
        number: 1,
        spec: MonitorSpec::Heartbeat,
        schedule: MonitorSchedule::Interval { every_seconds: 60 },
        timeout_millis: 10_000,
        max_retries: 0,
        consecutive_failures: 1,
        consecutive_recoveries: 1,
        freshness_seconds: 120,
        location_policy: MultiLocationPolicy::Any,
        location_ids: Vec::new(),
        escalation_policy_id: escalation_policy_id.map(Id::from_string),
        alert_on_degraded: true,
        alert_on_flaky: false,
        alert_on_no_data: true,
        inject_trace_context: true,
        traceroute_on_failure: true,
        last_test_result_id: None,
        last_test_passed_at: None,
        created_by: Id::from_string("user-1"),
        created_at: TimestampMicros(0),
        content_hash: String::new(),
    }
}

fn synthetic_transition(previous: MonitorState, current: MonitorState) -> SyntheticStateTransition {
    SyntheticStateTransition {
        id: Id::new(),
        organization_id: Id::from_string("org-a"),
        monitor_id: Id::from_string("monitor-1"),
        monitor_revision_id: Id::from_string("rev-1"),
        location_id: None,
        previous_state: previous,
        current_state: current,
        correlation_key: "synthetic:monitor-1".into(),
        result_id: Some(Id::from_string("result-1")),
        started_at: TimestampMicros(0),
        ended_at: None,
        created_at: TimestampMicros(0),
    }
}

#[tokio::test]
async fn a_failing_check_opens_one_incident_that_follows_its_state_until_it_recovers() {
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let evaluator = RuleEvaluator::new(
        Arc::new(InMemRules {
            inner: StdMutex::new(Vec::new()),
        }),
        incidents.clone() as Arc<dyn IncidentRepository>,
        Arc::new(InMemEvalState::default()),
        Arc::new(GapEngine {
            rounds: StdMutex::new(Vec::new()),
        }),
        10,
    );
    let monitor = synthetic_monitor();
    let revision = synthetic_revision(Some("pol-1"));
    let now = TimestampMicros::now();

    // What each state alerts as, the way the status pages see it too.
    let settle = |previous: MonitorState, current: MonitorState| {
        let transition = synthetic_transition(previous, current);
        let alert = current
            .alert_activation(revision.alert_on_degraded, revision.alert_on_flaky)
            .and_then(|(active, severity)| active.then_some(severity));
        (transition, alert)
    };

    let (transition, alert) = settle(MonitorState::Healthy, MonitorState::Failing);
    evaluator
        .settle_synthetic(&monitor, &revision, &transition, alert, now)
        .await
        .unwrap();
    {
        let g = incidents.inner.lock().unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].rule_id, Id::from_string("monitor-1"));
        assert_eq!(g[0].summary, "Checkout API");
        assert_eq!(g[0].severity, Severity::Error);
        assert_eq!(g[0].escalation_policy_id, Id::from_string("pol-1"));
        assert_eq!(g[0].labels["alert.source"], "synthetic");
        assert_eq!(g[0].labels["synthetic.state"], "failing");
        assert_eq!(g[0].labels["synthetic.kind"], "http");
        assert_eq!(g[0].labels["team_id"], "team-1");
        assert_eq!(g[0].labels["synthetic.tags"], "production,checkout");
        assert_eq!(g[0].annotations["synthetic.result_id"], "result-1");
        assert_eq!(
            g[0].synthetic_monitor_id(),
            Some(&Id::from_string("monitor-1"))
        );
    }

    // Degraded with its switch on: the same incident, a warning now.
    let (transition, alert) = settle(MonitorState::Failing, MonitorState::Degraded);
    evaluator
        .settle_synthetic(&monitor, &revision, &transition, alert, now)
        .await
        .unwrap();
    {
        let g = incidents.inner.lock().unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].severity, Severity::Warning);
        assert_eq!(g[0].labels["synthetic.state"], "degraded");
    }

    // Healthy again resolves it, and the next failure opens another.
    let (transition, alert) = settle(MonitorState::Degraded, MonitorState::Healthy);
    evaluator
        .settle_synthetic(&monitor, &revision, &transition, alert, now)
        .await
        .unwrap();
    assert_eq!(
        incidents.inner.lock().unwrap()[0].status,
        IncidentStatus::Resolved
    );

    let (transition, alert) = settle(MonitorState::Healthy, MonitorState::Failing);
    evaluator
        .settle_synthetic(&monitor, &revision, &transition, alert, now)
        .await
        .unwrap();
    {
        let g = incidents.inner.lock().unwrap();
        assert_eq!(g.len(), 2);
        assert_eq!(g[1].status, IncidentStatus::Open);
    }
}

#[tokio::test]
async fn a_check_without_an_escalation_policy_still_opens_its_incident() {
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let evaluator = RuleEvaluator::new(
        Arc::new(InMemRules {
            inner: StdMutex::new(Vec::new()),
        }),
        incidents.clone() as Arc<dyn IncidentRepository>,
        Arc::new(InMemEvalState::default()),
        Arc::new(GapEngine {
            rounds: StdMutex::new(Vec::new()),
        }),
        10,
    );

    evaluator
        .settle_synthetic(
            &synthetic_monitor(),
            &synthetic_revision(None),
            &synthetic_transition(MonitorState::Healthy, MonitorState::Failing),
            Some(Severity::Error),
            TimestampMicros::now(),
        )
        .await
        .unwrap();

    let g = incidents.inner.lock().unwrap();
    assert_eq!(g.len(), 1);
    // No policy to page through: the notify policies still deliver it.
    assert_eq!(g[0].escalation_policy_id, Id::from_string(""));
}

fn synthetic_evaluator() -> (Arc<InMemIncidents>, RuleEvaluator) {
    let incidents: Arc<InMemIncidents> = Arc::new(InMemIncidents::default());
    let evaluator = RuleEvaluator::new(
        Arc::new(InMemRules {
            inner: StdMutex::new(Vec::new()),
        }),
        incidents.clone() as Arc<dyn IncidentRepository>,
        Arc::new(InMemEvalState::default()),
        Arc::new(GapEngine {
            rounds: StdMutex::new(Vec::new()),
        }),
        10,
    );

    (incidents, evaluator)
}

/// Frankfurt missed three runs in a row; the stretch without results began at `since`.
fn no_data_change(no_data: bool, since: TimestampMicros) -> NoDataChange {
    NoDataChange {
        monitor: synthetic_monitor(),
        revision: synthetic_revision(Some("pol-1")),
        no_data,
        locations: vec!["Frankfurt".into()],
        missed_runs: 3,
        skip_reason: Some("no_agent_online".into()),
        since,
    }
}

const NO_DATA_TEXT: &str =
    "检查 Checkout API 没有结果：法兰克福 连续 3 次没有运行，那里没有在线的 Agent。";

#[tokio::test]
async fn a_check_without_results_opens_a_warning_beside_its_state_until_results_return() {
    let (incidents, evaluator) = synthetic_evaluator();
    let now = TimestampMicros::now();

    evaluator
        .settle_synthetic_no_data(
            &no_data_change(true, TimestampMicros(0)),
            async || Some(NO_DATA_TEXT.to_string()),
            now,
        )
        .await
        .unwrap();
    {
        let g = incidents.inner.lock().unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].severity, Severity::Warning);
        assert_eq!(g[0].summary, "Checkout API");
        assert_eq!(g[0].escalation_policy_id, Id::from_string("pol-1"));
        assert_eq!(g[0].labels["synthetic.state"], "no_data");
        assert_eq!(g[0].annotations["description"], NO_DATA_TEXT);
        assert!(g[0].is_no_data());
        assert_eq!(
            g[0].synthetic_monitor_id(),
            Some(&Id::from_string("monitor-1"))
        );
    }

    // Told again while it lasts: still the one incident.
    evaluator
        .settle_synthetic_no_data(
            &no_data_change(true, TimestampMicros(0)),
            async || None,
            now,
        )
        .await
        .unwrap();
    assert_eq!(incidents.inner.lock().unwrap().len(), 1);

    // The check failing as well opens its state incident beside it.
    evaluator
        .settle_synthetic(
            &synthetic_monitor(),
            &synthetic_revision(Some("pol-1")),
            &synthetic_transition(MonitorState::Healthy, MonitorState::Failing),
            Some(Severity::Error),
            now,
        )
        .await
        .unwrap();
    {
        let g = incidents.inner.lock().unwrap();
        assert_eq!(g.len(), 2);
        assert_ne!(g[0].fingerprint, g[1].fingerprint);
        assert!(!g[1].is_no_data());
    }

    // Results again resolve the no-data incident; the state incident follows the state.
    evaluator
        .settle_synthetic_no_data(
            &no_data_change(false, TimestampMicros(0)),
            async || None,
            now,
        )
        .await
        .unwrap();

    let g = incidents.inner.lock().unwrap();
    assert_eq!(g[0].status, IncidentStatus::Resolved);
    assert_eq!(g[1].status, IncidentStatus::Open);
}

#[tokio::test]
async fn a_no_data_incident_someone_resolved_stays_resolved_while_the_check_has_none() {
    let (incidents, evaluator) = synthetic_evaluator();
    let now = TimestampMicros::now();

    evaluator
        .settle_synthetic_no_data(
            &no_data_change(true, TimestampMicros(0)),
            async || None,
            now,
        )
        .await
        .unwrap();
    {
        let mut g = incidents.inner.lock().unwrap();
        g[0].status = IncidentStatus::Resolved;
        g[0].resolved_at = Some(now);
        g[0].resolved_by = Some(Id::from_string("user-1"));
    }

    // The same stretch without results: not opened again.
    evaluator
        .settle_synthetic_no_data(
            &no_data_change(true, TimestampMicros(0)),
            async || None,
            now,
        )
        .await
        .unwrap();
    assert_eq!(incidents.inner.lock().unwrap().len(), 1);

    // A stretch that began after it opens another.
    let later = TimestampMicros(now.0 + 1);
    evaluator
        .settle_synthetic_no_data(&no_data_change(true, later), async || None, later)
        .await
        .unwrap();

    let g = incidents.inner.lock().unwrap();
    assert_eq!(g.len(), 2);
    assert_eq!(g[1].status, IncidentStatus::Open);
    // Without the words for it, the incident says no more than its labels do.
    assert!(!g[1].annotations.contains_key("description"));
}

#[tokio::test]
async fn pausing_or_archiving_a_check_resolves_its_incidents() {
    let (incidents, evaluator) = synthetic_evaluator();
    let now = TimestampMicros::now();
    let monitor = synthetic_monitor();

    evaluator
        .settle_synthetic(
            &monitor,
            &synthetic_revision(Some("pol-1")),
            &synthetic_transition(MonitorState::Healthy, MonitorState::Failing),
            Some(Severity::Error),
            now,
        )
        .await
        .unwrap();
    evaluator
        .settle_synthetic_no_data(
            &no_data_change(true, TimestampMicros(0)),
            async || None,
            now,
        )
        .await
        .unwrap();

    // Acknowledged or not, no run is left to resolve it.
    incidents.inner.lock().unwrap()[0].status = IncidentStatus::Acknowledged;

    evaluator.close_synthetic(&monitor, now).await.unwrap();

    let g = incidents.inner.lock().unwrap();
    assert_eq!(g.len(), 2);
    assert!(
        g.iter()
            .all(|incident| incident.status == IncidentStatus::Resolved
                && incident.resolved_by.is_none())
    );
}
