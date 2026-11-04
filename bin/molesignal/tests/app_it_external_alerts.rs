// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 外部告警端到端（内存仓库，不连数据库）：告警来源的 Token、各家格式之一读进来以后怎样开事件、
//! 跟着分组更新、在来源报恢复时解决（已确认的也解决），晚到的旧消息、同时到达的重复消息、
//! 停用的来源，以及联邦里转给接收集群的请求。

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex as StdMutex},
};

use async_trait::async_trait;
use molesignal::{
    app::alerting::{
        AlertSources, AlertingService, RuleEvaluator,
        sources::{
            AlertForwarder, AlertSourcesParts, FederationCluster, ForwardedRequest, IntakeReply,
            SourceWrite,
        },
    },
    domain::{
        alerting::{
            escalation::EscalationPolicy,
            incident::{Incident, IncidentStatus, Severity},
            repositories::{
                AlertRuleEvalState, AlertRuleEvalStateRepository, AlertRuleRepository,
                EscalationPolicyRepository, IncidentRepository, ScheduleRepository,
            },
            rule::{AlertRule, RuleState},
            schedule::Schedule,
            source::{
                AlertSource, AlertSourceKind, AlertSourceRepository, EmailRules, IntakeRequest,
                IntakeRequestRepository, IntakeStatus, KeyLock, MailCursor, Mailbox,
                MailboxRepository, SourceIncident, SourceIncidentRepository,
            },
            timeline::ActionVia,
        },
        query::{QueryEngine, QueryRequest, QueryResult},
    },
    infra::cluster::{RemoteCluster, RemoteClustersRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};
use serde_json::{Value, json};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

// ---- 内存仓库 ----

#[derive(Default)]
struct Incidents(StdMutex<Vec<Incident>>);

fn active(status: IncidentStatus) -> bool {
    matches!(status, IncidentStatus::Open | IncidentStatus::Acknowledged)
}

#[async_trait]
impl IncidentRepository for Incidents {
    async fn create(&self, incident: Incident) -> Result<Incident> {
        let mut all = self.0.lock().unwrap();

        // 库里的唯一索引：同一指纹同时只有一个活跃事件。
        if all
            .iter()
            .any(|other| other.fingerprint == incident.fingerprint && active(other.status))
        {
            return Err(Error::conflict("duplicate active fingerprint"));
        }

        all.push(incident.clone());
        Ok(incident)
    }

    async fn update(&self, incident: Incident) -> Result<Incident> {
        let mut all = self.0.lock().unwrap();

        if let Some(slot) = all.iter_mut().find(|other| other.id == incident.id) {
            *slot = incident.clone();
        }

        Ok(incident)
    }

    async fn update_if_status(
        &self,
        incident: Incident,
        expected: &[IncidentStatus],
    ) -> Result<Option<Incident>> {
        let mut all = self.0.lock().unwrap();

        match all.iter_mut().find(|other| other.id == incident.id) {
            Some(slot) if expected.contains(&slot.status) && slot.revision == incident.revision => {
                let written = Incident {
                    revision: incident.revision + 1,
                    ..incident
                };
                *slot = written.clone();
                Ok(Some(written))
            }
            _ => Ok(None),
        }
    }

    async fn list_merged_into(&self, _: &Id, target: &Id) -> Result<Vec<Incident>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|incident| incident.handling.merged_into.as_ref() == Some(target))
            .cloned()
            .collect())
    }

    async fn get(&self, id: &Id) -> Result<Incident> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .find(|incident| &incident.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("incident"))
    }

    async fn list_active(&self, org_id: &Id) -> Result<Vec<Incident>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|incident| &incident.org_id == org_id && active(incident.status))
            .cloned()
            .collect())
    }

    async fn list_active_for_rule(&self, org_id: &Id, rule_id: &Id) -> Result<Vec<Incident>> {
        Ok(self
            .list_active(org_id)
            .await?
            .into_iter()
            .filter(|incident| &incident.rule_id == rule_id)
            .collect())
    }

    async fn find_by_fingerprint(
        &self,
        org_id: &Id,
        fingerprint: &str,
    ) -> Result<Option<Incident>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|incident| &incident.org_id == org_id && incident.fingerprint == fingerprint)
            .max_by_key(|incident| incident.created_at.0)
            .cloned())
    }

    async fn list_by_status(&self, org_id: &Id, status: IncidentStatus) -> Result<Vec<Incident>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|incident| &incident.org_id == org_id && incident.status == status)
            .cloned()
            .collect())
    }

    async fn list_since(&self, org_id: &Id, _since: TimestampMicros) -> Result<Vec<Incident>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|incident| &incident.org_id == org_id)
            .cloned()
            .collect())
    }
}

#[derive(Default)]
struct Sources(StdMutex<Vec<AlertSource>>);

#[async_trait]
impl AlertSourceRepository for Sources {
    async fn create(&self, source: AlertSource) -> Result<AlertSource> {
        self.0.lock().unwrap().push(source.clone());
        Ok(source)
    }

    async fn update(&self, source: AlertSource) -> Result<AlertSource> {
        let mut all = self.0.lock().unwrap();

        if let Some(slot) = all.iter_mut().find(|other| other.id == source.id) {
            let last_intake_at = slot.last_intake_at;
            *slot = source.clone();
            slot.last_intake_at = last_intake_at;
        }

        Ok(source)
    }

    async fn get(&self, id: &Id) -> Result<AlertSource> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .find(|source| &source.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("alert source"))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<AlertSource>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|source| &source.org_id == org_id)
            .cloned()
            .collect())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.0.lock().unwrap().retain(|source| &source.id != id);
        Ok(())
    }

    async fn find_by_token_prefix(&self, prefix: &str) -> Result<Option<AlertSource>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|source| source.token_prefix == prefix)
            .cloned())
    }

    async fn touch_intake(&self, id: &Id, at: TimestampMicros) -> Result<()> {
        if let Some(source) = self
            .0
            .lock()
            .unwrap()
            .iter_mut()
            .find(|source| &source.id == id)
        {
            source.last_intake_at = Some(at);
        }

        Ok(())
    }
}

#[derive(Default)]
struct Requests(StdMutex<Vec<IntakeRequest>>);

#[async_trait]
impl IntakeRequestRepository for Requests {
    async fn record(&self, request: IntakeRequest) -> Result<()> {
        self.0.lock().unwrap().push(request);
        Ok(())
    }

    async fn list(&self, org_id: &Id, source_id: &Id, limit: u32) -> Result<Vec<IntakeRequest>> {
        let all = self.0.lock().unwrap();

        Ok(all
            .iter()
            .rev()
            .filter(|request| &request.org_id == org_id && &request.source_id == source_id)
            .take(limit as usize)
            .cloned()
            .collect())
    }
}

/// 来源侧的状态，加上按键的锁（数据库里是 advisory lock）。
#[derive(Default)]
struct SourceIncidents {
    rows: StdMutex<HashMap<String, SourceIncident>>,
    locks: StdMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

/// 锁在它被丢弃时放开。
struct HeldLock {
    _guard: OwnedMutexGuard<()>,
}

#[async_trait]
impl KeyLock for HeldLock {
    async fn release(self: Box<Self>) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl SourceIncidentRepository for SourceIncidents {
    async fn get(&self, incident_id: &Id) -> Result<Option<SourceIncident>> {
        Ok(self.rows.lock().unwrap().get(&incident_id.0).cloned())
    }

    async fn upsert(&self, row: SourceIncident) -> Result<()> {
        let mut rows = self.rows.lock().unwrap();

        let last_event_at = rows
            .get(&row.incident_id.0)
            .and_then(|old| old.last_event_at)
            .max(row.last_event_at);

        rows.insert(
            row.incident_id.0.clone(),
            SourceIncident {
                last_event_at,
                ..row
            },
        );

        Ok(())
    }

    async fn lock(&self, key: &str) -> Result<Box<dyn KeyLock>> {
        let mutex = self
            .locks
            .lock()
            .unwrap()
            .entry(key.to_string())
            .or_default()
            .clone();

        Ok(Box::new(HeldLock {
            _guard: mutex.lock_owned().await,
        }))
    }
}

struct Policies(Vec<EscalationPolicy>);

#[async_trait]
impl EscalationPolicyRepository for Policies {
    async fn create(&self, policy: EscalationPolicy) -> Result<EscalationPolicy> {
        Ok(policy)
    }

    async fn update(&self, policy: EscalationPolicy) -> Result<EscalationPolicy> {
        Ok(policy)
    }

    async fn get(&self, id: &Id) -> Result<EscalationPolicy> {
        self.0
            .iter()
            .find(|policy| &policy.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("policy"))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<EscalationPolicy>> {
        Ok(self
            .0
            .iter()
            .filter(|policy| &policy.org_id == org_id)
            .cloned()
            .collect())
    }

    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

struct NoMailboxes;

#[async_trait]
impl MailboxRepository for NoMailboxes {
    async fn get(&self, _source_id: &Id) -> Result<Option<Mailbox>> {
        Ok(None)
    }

    async fn list_all(&self) -> Result<Vec<Mailbox>> {
        Ok(Vec::new())
    }

    async fn save(&self, mailbox: Mailbox) -> Result<Mailbox> {
        Ok(mailbox)
    }

    async fn delete(&self, _source_id: &Id) -> Result<()> {
        Ok(())
    }

    async fn advance(&self, _source_id: &Id, _cursor: MailCursor) -> Result<()> {
        Ok(())
    }

    async fn record_poll(
        &self,
        _source_id: &Id,
        _at: TimestampMicros,
        _error: Option<&str>,
    ) -> Result<()> {
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

struct NoRules;

#[async_trait]
impl AlertRuleRepository for NoRules {
    async fn create(&self, rule: AlertRule) -> Result<AlertRule> {
        Ok(rule)
    }

    async fn update(&self, rule: AlertRule) -> Result<AlertRule> {
        Ok(rule)
    }

    async fn get(&self, _id: &Id) -> Result<AlertRule> {
        Err(Error::not_found("rule"))
    }

    async fn list(&self, _org_id: &Id) -> Result<Vec<AlertRule>> {
        Ok(Vec::new())
    }

    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }

    async fn list_enabled(&self) -> Result<Vec<AlertRule>> {
        Ok(Vec::new())
    }

    async fn record_evaluation(
        &self,
        _id: &Id,
        _evaluated_at: TimestampMicros,
        _state: RuleState,
    ) -> Result<()> {
        Ok(())
    }
}

struct NoEvalState;

#[async_trait]
impl AlertRuleEvalStateRepository for NoEvalState {
    async fn claim_cycle(&self, _: &Id, _: TimestampMicros, _: i64) -> Result<bool> {
        Ok(true)
    }

    async fn upsert_state(&self, state: AlertRuleEvalState) -> Result<AlertRuleEvalState> {
        Ok(state)
    }

    async fn get(&self, _: &Id, _: &str) -> Result<Option<AlertRuleEvalState>> {
        Ok(None)
    }

    async fn list_groups(&self, _: &Id) -> Result<Vec<AlertRuleEvalState>> {
        Ok(Vec::new())
    }

    async fn reset(&self, _: &Id, _: &str) -> Result<()> {
        Ok(())
    }

    async fn forget_groups(&self, _: &Id, _: &[String]) -> Result<()> {
        Ok(())
    }
}

struct NoQueries;

#[async_trait]
impl QueryEngine for NoQueries {
    async fn execute(&self, _request: QueryRequest) -> Result<QueryResult> {
        Err(Error::internal("external alerts run no query"))
    }
}

/// 本集群在联邦里的 ID，测试里可以改。
#[derive(Default)]
struct Cluster(StdMutex<String>);

#[async_trait]
impl FederationCluster for Cluster {
    async fn cluster_id(&self) -> Result<String> {
        Ok(self.0.lock().unwrap().clone())
    }
}

/// 联邦里的远端集群：只认启用的那几个。
struct RemoteClusters(Vec<&'static str>);

#[async_trait]
impl RemoteClustersRepository for RemoteClusters {
    async fn create(&self, cluster: RemoteCluster) -> Result<RemoteCluster> {
        Ok(cluster)
    }

    async fn update(&self, cluster: RemoteCluster) -> Result<RemoteCluster> {
        Ok(cluster)
    }

    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }

    async fn get(&self, id: &Id) -> Result<RemoteCluster> {
        self.list_enabled()
            .await?
            .into_iter()
            .find(|cluster| &cluster.id == id)
            .ok_or_else(|| Error::not_found("cluster"))
    }

    async fn list(&self) -> Result<Vec<RemoteCluster>> {
        self.list_enabled().await
    }

    async fn list_enabled(&self) -> Result<Vec<RemoteCluster>> {
        Ok(self
            .0
            .iter()
            .map(|id| RemoteCluster {
                id: Id::from_string(*id),
                name: format!("{id} name"),
                advertise_addr: format!("https://{id}.example.com"),
                token_secret_ref: "env:TOKEN".into(),
                tls_verify: true,
                enabled: true,
                discovered: false,
                created_at: TimestampMicros(1),
                updated_at: TimestampMicros(1),
            })
            .collect())
    }

    async fn insert_discovered(&self, _: &str, _: &str, _: &str) -> Result<u64> {
        Ok(0)
    }
}

/// 记下转出去的请求；`reply` 为空时当作连不上接收集群。
#[derive(Default)]
struct Forwarder {
    reply: StdMutex<Option<IntakeReply>>,
    sent: StdMutex<Vec<(String, String, String)>>,
}

#[async_trait]
impl AlertForwarder for Forwarder {
    async fn forward(&self, request: ForwardedRequest<'_>) -> Result<IntakeReply> {
        self.sent.lock().unwrap().push((
            request.home_cluster.to_string(),
            request.local_cluster.to_string(),
            request.source_id.0.clone(),
        ));

        self.reply
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| Error::unavailable("connection refused"))
    }
}

// ---- 场景 ----

struct World {
    sources: Arc<AlertSources>,
    alerting: Arc<AlertingService>,
    incidents: Arc<Incidents>,
    requests: Arc<Requests>,
    cluster: Arc<Cluster>,
    forwarder: Arc<Forwarder>,
}

fn org() -> Id {
    Id::from_string("org-a")
}

fn policy() -> EscalationPolicy {
    EscalationPolicy {
        id: Id::from_string("pol-1"),
        org_id: org(),
        name: "default".into(),
        steps: Vec::new(),
        repeat: false,
        max_loops: 1,
        ack_retrigger_secs: None,
        urgency_rule: Default::default(),
        escalate_low_urgency: false,
    }
}

fn world() -> World {
    let incidents = Arc::new(Incidents::default());
    let requests = Arc::new(Requests::default());
    let cluster = Arc::new(Cluster::default());
    let forwarder = Arc::new(Forwarder::default());
    let policies = Arc::new(Policies(vec![policy()]));

    let alerting = Arc::new(AlertingService::new(
        Arc::new(NoRules),
        incidents.clone(),
        Arc::new(NoSchedules),
        policies.clone(),
    ));

    let evaluator = Arc::new(RuleEvaluator::new(
        Arc::new(NoRules),
        incidents.clone(),
        Arc::new(NoEvalState),
        Arc::new(NoQueries),
        5,
    ));

    let sources = AlertSources::new(AlertSourcesParts {
        sources: Arc::new(Sources::default()),
        mailboxes: Arc::new(NoMailboxes),
        requests: requests.clone(),
        source_incidents: Arc::new(SourceIncidents::default()),
        incidents: incidents.clone(),
        escalations: policies,
        evaluator,
        alerting: alerting.clone(),
        cluster: cluster.clone(),
        remote_clusters: Arc::new(RemoteClusters(vec!["cluster-a", "cluster-b"])),
    })
    .with_forwarder(forwarder.clone());

    World {
        sources: Arc::new(sources),
        alerting,
        incidents,
        requests,
        cluster,
        forwarder,
    }
}

fn write(name: &str, policy: Option<&str>) -> SourceWrite {
    SourceWrite {
        name: name.into(),
        enabled: true,
        escalation_policy_id: policy.map(Id::from_string),
        severity: Severity::Warning,
        labels: BTreeMap::from([("env".to_string(), "prod".to_string())]),
        home_cluster: None,
        email: None,
    }
}

impl World {
    async fn source(&self, kind: AlertSourceKind, policy: Option<&str>) -> (AlertSource, String) {
        self.sources
            .create(
                &org(),
                &Id::from_string("user-1"),
                kind,
                write("Prometheus", policy),
            )
            .await
            .unwrap()
    }

    async fn send(&self, token: &str, body: &Value) -> IntakeReply {
        let source = self.sources.authenticate(Some(token)).await.unwrap();
        let body = serde_json::to_vec(body).unwrap();
        self.sources
            .intake(&source, Some("application/json"), &body)
            .await
    }

    fn all(&self) -> Vec<Incident> {
        let mut all = self.incidents.0.lock().unwrap().clone();
        all.sort_by_key(|incident| incident.created_at.0);
        all
    }

    fn last_request(&self) -> IntakeRequest {
        self.requests.0.lock().unwrap().last().cloned().unwrap()
    }
}

fn outcome(reply: &IntakeReply) -> &str {
    reply.body["alerts"][0]["outcome"]
        .as_str()
        .unwrap_or_default()
}

fn member(instance: &str, severity: &str, starts_at: &str) -> Value {
    json!({
        "status": "firing",
        "labels": {"alertname": "HighCPU", "instance": instance, "job": "node", "severity": severity},
        "annotations": {"summary": format!("CPU above 90% on {instance}")},
        "startsAt": starts_at,
        "endsAt": "0001-01-01T00:00:00Z",
        "generatorURL": "http://prometheus.example.com:9090/graph",
        "fingerprint": instance
    })
}

fn notification(status: &str, alerts: Vec<Value>) -> Value {
    json!({
        "version": "4",
        "groupKey": "{}:{alertname=\"HighCPU\"}",
        "status": status,
        "receiver": "molesignal",
        "groupLabels": {"alertname": "HighCPU"},
        "commonLabels": {"alertname": "HighCPU", "job": "node"},
        "commonAnnotations": {},
        "externalURL": "http://alertmanager.example.com:9093",
        "alerts": alerts
    })
}

fn resolved(instance: &str, starts_at: &str, ends_at: &str) -> Value {
    let mut alert = member(instance, "warning", starts_at);
    alert["status"] = json!("resolved");
    alert["endsAt"] = json!(ends_at);
    alert
}

#[tokio::test]
async fn an_alertmanager_group_opens_one_incident_that_follows_the_group() {
    let world = world();
    let (source, token) = world
        .source(AlertSourceKind::Alertmanager, Some("pol-1"))
        .await;

    let first = notification(
        "firing",
        vec![
            member("web-1", "warning", "2026-10-27T08:00:00Z"),
            member("web-2", "warning", "2026-10-27T08:01:00Z"),
        ],
    );

    let reply = world.send(&token, &first).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(outcome(&reply), "opened");

    let incident = world.all().remove(0);
    assert_eq!(incident.rule_id, source.id);
    assert_eq!(incident.escalation_policy_id, Id::from_string("pol-1"));
    assert_eq!(incident.status, IncidentStatus::Open);
    assert_eq!(incident.severity, Severity::Warning);
    assert_eq!(incident.summary, "HighCPU");
    assert_eq!(incident.labels["alert.source"], "external");
    assert_eq!(incident.labels["external.kind"], "alertmanager");
    assert_eq!(incident.labels["env"], "prod");
    assert_eq!(incident.labels["job"], "node");
    assert_eq!(incident.external_source_id(), Some(&source.id));

    let state = world
        .sources
        .incident_source(&incident)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.state.members.len(), 2);
    assert!(state.source.is_some());

    // 整组重发：同一个事件，什么都没变。
    assert_eq!(outcome(&world.send(&token, &first).await), "unchanged");

    // 组里多了一条：分组成员变了。
    let grown = notification(
        "firing",
        vec![
            member("web-1", "warning", "2026-10-27T08:00:00Z"),
            member("web-2", "warning", "2026-10-27T08:01:00Z"),
            member("web-3", "warning", "2026-10-27T08:02:00Z"),
        ],
    );

    assert_eq!(outcome(&world.send(&token, &grown).await), "updated");

    // 有一条升到 critical：事件跟着升级，从第一步重来。
    let critical = notification(
        "firing",
        vec![
            member("web-1", "warning", "2026-10-27T08:00:00Z"),
            member("web-2", "critical", "2026-10-27T08:01:00Z"),
            member("web-3", "warning", "2026-10-27T08:02:00Z"),
        ],
    );

    assert_eq!(outcome(&world.send(&token, &critical).await), "updated");
    let incident = world.all().remove(0);
    assert_eq!(incident.severity, Severity::Critical);
    assert_eq!(incident.current_step, 0);
    assert_eq!(world.all().len(), 1);

    // 有人确认了，之后 Alertmanager 报整组恢复：已确认的也解决，解决人为空。
    world
        .alerting
        .acknowledge(
            &incident.id,
            Id::from_string("user-1"),
            TimestampMicros::now(),
            ActionVia::Web,
        )
        .await
        .unwrap();

    let recovered = notification(
        "resolved",
        vec![
            resolved("web-1", "2026-10-27T08:00:00Z", "2026-10-27T09:00:00Z"),
            resolved("web-2", "2026-10-27T08:01:00Z", "2026-10-27T09:01:00Z"),
            resolved("web-3", "2026-10-27T08:02:00Z", "2026-10-27T09:02:00Z"),
        ],
    );

    assert_eq!(outcome(&world.send(&token, &recovered).await), "resolved");
    let incident = world.all().remove(0);
    assert_eq!(incident.status, IncidentStatus::Resolved);
    assert_eq!(incident.resolved_by, None);

    // 恢复之前发出、之后才到的触发：晚到的旧消息，不开僵尸事件。
    let late = world.send(&token, &critical).await;
    assert_eq!(outcome(&late), "ignored");
    assert_eq!(world.all().len(), 1);

    // 真正的下一次：开新事件。
    let again = notification(
        "firing",
        vec![member("web-1", "warning", "2026-10-27T10:00:00Z")],
    );
    assert_eq!(outcome(&world.send(&token, &again).await), "opened");
    assert_eq!(world.all().len(), 2);
    assert_eq!(world.last_request().status, IntakeStatus::Accepted);
}

#[tokio::test]
async fn the_api_triggers_acknowledges_and_resolves_by_dedup_key() {
    let world = world();
    let (_, token) = world.source(AlertSourceKind::Api, None).await;

    let trigger = json!({"dedup_key": "disk/web-1", "summary": "Disk full on web-1",
                         "severity": "error", "labels": {"host.name": "web-1"}});

    assert_eq!(outcome(&world.send(&token, &trigger).await), "opened");

    let incident = world.all().remove(0);
    assert_eq!(incident.escalation_policy_id, Id::from_string(""));
    assert_eq!(incident.host_ids, vec!["web-1".to_string()]);
    assert_eq!(incident.severity, Severity::Error);

    let acknowledge = json!({"action": "acknowledge", "dedup_key": "disk/web-1"});
    assert_eq!(
        outcome(&world.send(&token, &acknowledge).await),
        "acknowledged"
    );

    let incident = world.all().remove(0);
    assert_eq!(incident.status, IncidentStatus::Acknowledged);
    assert_eq!(incident.acknowledged_by, None);

    assert_eq!(outcome(&world.send(&token, &trigger).await), "unchanged");

    let resolve = json!({"action": "resolve", "dedup_key": "disk/web-1"});
    assert_eq!(outcome(&world.send(&token, &resolve).await), "resolved");
    assert_eq!(outcome(&world.send(&token, &resolve).await), "ignored");

    let batch = json!([
        {"dedup_key": "a", "summary": "A"},
        {"dedup_key": "b", "summary": "B"}
    ]);

    let reply = world.send(&token, &batch).await;
    assert_eq!(reply.body["alerts"].as_array().unwrap().len(), 2);
    assert_eq!(world.all().len(), 3);
}

#[tokio::test]
async fn a_zabbix_problem_follows_its_updates_and_its_recovery() {
    let world = world();
    let (_, token) = world.source(AlertSourceKind::Zabbix, None).await;

    let event = |overrides: Value| {
        let mut event = json!({
            "event_id": "3148",
            "event_value": "1",
            "event_name": "High CPU on web-1",
            "event_time": "1761552000",
            "recovery_time": "{EVENT.RECOVERY.TIMESTAMP}",
            "update_status": "0",
            "update_action": "{EVENT.UPDATE.ACTION}",
            "update_time": "{EVENT.UPDATE.TIMESTAMP}",
            "ack_status": "No",
            "severity": "4",
            "host_name": "web-1",
            "zabbix_url": "https://zabbix.acme.com"
        });

        for (key, value) in overrides.as_object().unwrap() {
            event[key] = value.clone();
        }

        event
    };

    assert_eq!(
        outcome(&world.send(&token, &event(json!({}))).await),
        "opened"
    );

    let incident = world.all().remove(0);
    assert_eq!(incident.severity, Severity::Error);
    assert_eq!(incident.summary, "High CPU on web-1");
    assert_eq!(incident.labels["zabbix.event_id"], "3148");

    // A problem somebody acknowledged in Zabbix stays one incident, acknowledged by nobody here.
    let acknowledged = event(json!({
        "update_status": "1",
        "update_action": "acknowledged",
        "update_time": "1761552300",
        "ack_status": "Yes"
    }));
    assert_eq!(
        outcome(&world.send(&token, &acknowledged).await),
        "acknowledged"
    );
    assert_eq!(world.all().remove(0).status, IncidentStatus::Acknowledged);

    // Its macro left as it is, as in the media type's Test button: nothing to go on.
    let unexpanded = world
        .send(&token, &event(json!({"event_id": "{EVENT.ID}"})))
        .await;
    assert_eq!(unexpanded.status, 400);

    let recovered = event(json!({"event_value": "0", "recovery_time": "1761552600"}));
    assert_eq!(outcome(&world.send(&token, &recovered).await), "resolved");

    let all = world.all();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].status, IncidentStatus::Resolved);
}

#[tokio::test]
async fn a_mail_opens_an_incident_its_recovery_mail_resolves() {
    let world = world();
    let (source, token) = world.source(AlertSourceKind::Email, None).await;

    let mut listed = write("Prometheus", None);
    listed.email = Some(EmailRules {
        allowed_senders: vec!["@monitor.acme.com".into()],
        ..EmailRules::default()
    });
    world
        .sources
        .update(&org(), &source.id, listed)
        .await
        .unwrap();

    let source = world.sources.authenticate(Some(&token)).await.unwrap();
    let mail = |from: &str, subject: &str| {
        format!(
            "From: Monitor <{from}>\r\nTo: alerts@acme.com\r\nSubject: {subject}\r\n\
             Date: Mon, 27 Oct 2025 00:00:00 +0000\r\nMessage-ID: <m1@acme.com>\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\nDisk is full\r\n"
        )
    };
    let send = |raw: String| {
        let (world, source) = (&world, &source);

        async move {
            world
                .sources
                .intake(source, Some("message/rfc822"), raw.as_bytes())
                .await
        }
    };

    let stranger = send(mail(
        "stranger@elsewhere.org",
        "PROBLEM: disk full on web-1",
    ))
    .await;
    assert_eq!(stranger.status, 400);
    assert_eq!(world.last_request().status, IntakeStatus::Rejected);
    assert!(world.all().is_empty());

    let opened = send(mail(
        "nagios@monitor.acme.com",
        "CRITICAL: disk full on web-1 (91%)",
    ))
    .await;
    assert_eq!(outcome(&opened), "opened");

    let incident = world.all().remove(0);
    assert_eq!(incident.severity, Severity::Critical);
    assert_eq!(incident.labels["email.from"], "nagios@monitor.acme.com");

    // Its time moves on, as the next mail's would: a mail no newer than the last counts as late.
    let raw = mail(
        "nagios@monitor.acme.com",
        "RECOVERY: disk full on web-1 (40%)",
    )
    .replace("00:00:00 +0000", "00:10:00 +0000");
    assert_eq!(outcome(&send(raw).await), "resolved");

    let all = world.all();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].status, IncidentStatus::Resolved);
}

#[tokio::test]
async fn tencent_cloud_alarms_open_and_recover() {
    let world = world();
    let (_, token) = world.source(AlertSourceKind::TencentMonitor, None).await;

    let alarm = |status: u8, recovered_at: &str| {
        json!({
            "sessionId": "session-1",
            "alarmStatus": status,
            "alarmType": "metric",
            "alarmLevel": "Serious",
            "alarmObjInfo": {
                "region": "gz",
                "uin": "1000",
                "dimensions": {"objName": "10.0.0.8(web-1)", "unInstanceId": "ins-1"}
            },
            "alarmPolicyInfo": {
                "policyId": "policy-1",
                "policyName": "CPU high",
                "conditions": {
                    "metricName": "cpu_usage",
                    "metricShowName": "CPU usage",
                    "calcType": ">",
                    "calcValue": "80",
                    "calcUnit": "%"
                }
            },
            "firstOccurTime": "2024-03-07 18:12:00",
            "durationTime": 0,
            "recoverTime": recovered_at
        })
    };

    assert_eq!(outcome(&world.send(&token, &alarm(1, "0")).await), "opened");

    let incident = world.all().remove(0);
    assert_eq!(incident.severity, Severity::Critical);
    assert_eq!(incident.summary, "CPU usage > 80% · 10.0.0.8(web-1)");

    let recovery = alarm(0, "2024-03-07 18:30:00");
    assert_eq!(outcome(&world.send(&token, &recovery).await), "resolved");
    assert_eq!(world.all().remove(0).status, IncidentStatus::Resolved);
}

#[tokio::test]
async fn cloudwatch_alarms_arrive_through_sns_and_follow_the_alarm_state() {
    let world = world();
    let (_, token) = world.source(AlertSourceKind::AwsCloudwatch, None).await;

    let notice = |state: &str, changed_at: &str| {
        let alarm = json!({
            "AlarmName": "checkout-cpu-high",
            "AWSAccountId": "123456789012",
            "NewStateValue": state,
            "NewStateReason": "Threshold Crossed",
            "StateChangeTime": changed_at,
            "Region": "US East (N. Virginia)",
            "AlarmArn": "arn:aws:cloudwatch:us-east-1:123456789012:alarm:checkout-cpu-high",
            "Trigger": {"MetricName": "CPUUtilization", "Namespace": "AWS/EC2", "Dimensions": []}
        });

        json!({
            "Type": "Notification",
            "TopicArn": "arn:aws:sns:us-east-1:123456789012:alerts",
            "Message": alarm.to_string(),
            "Timestamp": "2024-03-07T10:12:01.123Z"
        })
    };

    let alarm = notice("ALARM", "2024-03-07T10:12:00.000+0000");
    assert_eq!(outcome(&world.send(&token, &alarm).await), "opened");

    let incident = world.all().remove(0);
    assert_eq!(incident.summary, "checkout-cpu-high");
    assert_eq!(incident.severity, Severity::Warning);
    assert_eq!(incident.labels["aws.region"], "us-east-1");

    let ok = notice("OK", "2024-03-07T10:30:00.000+0000");
    assert_eq!(outcome(&world.send(&token, &ok).await), "resolved");
    assert_eq!(world.all().remove(0).status, IncidentStatus::Resolved);

    // The confirmation is answered at the address; here it only leaves a line in the record.
    let confirmation = json!({"Type": "SubscriptionConfirmation", "Message": "subscribe"});
    let reply = world.send(&token, &confirmation).await;
    assert_eq!(reply.status, 200);
    assert_eq!(world.last_request().status, IntakeStatus::Accepted);
    assert_eq!(world.all().len(), 1);

    // Raw message delivery leaves no signature to check.
    let raw = world
        .send(&token, &json!({"AlarmName": "x", "NewStateValue": "ALARM"}))
        .await;
    assert_eq!(raw.status, 400);
    assert_eq!(world.last_request().status, IntakeStatus::Rejected);
}

#[tokio::test]
async fn a_wrong_token_or_a_turned_off_source_is_refused() {
    let world = world();
    let (source, token) = world.source(AlertSourceKind::Api, None).await;

    assert!(matches!(
        world.sources.authenticate(None).await,
        Err(Error::Unauthorized(_))
    ));
    assert!(
        world
            .sources
            .authenticate(Some("mspat-nope"))
            .await
            .is_err()
    );
    assert!(world.sources.authenticate(Some(&token)).await.is_ok());

    let (_, rotated) = world
        .sources
        .rotate_token(&org(), &source.id)
        .await
        .unwrap();
    assert!(world.sources.authenticate(Some(&token)).await.is_err());
    assert!(world.sources.authenticate(Some(&rotated)).await.is_ok());

    let mut off = write("Prometheus", None);
    off.enabled = false;
    world.sources.update(&org(), &source.id, off).await.unwrap();

    let reply = world.send(&rotated, &json!({"summary": "x"})).await;
    assert_eq!(reply.status, 409);
    assert_eq!(world.last_request().status, IntakeStatus::Rejected);
    assert!(world.all().is_empty());
}

#[tokio::test]
async fn an_unreadable_body_is_refused_and_kept_for_a_look() {
    let world = world();
    let (source, _) = world.source(AlertSourceKind::Alertmanager, None).await;

    let reply = world.sources.intake(&source, None, b"not json").await;
    assert_eq!(reply.status, 400);

    let request = world.last_request();
    assert_eq!(request.status, IntakeStatus::Rejected);
    assert_eq!(request.body_excerpt.as_deref(), Some("not json"));
    assert!(request.message.unwrap().contains("Alertmanager"));
}

#[tokio::test]
async fn the_same_group_arriving_at_several_nodes_at_once_opens_one_incident() {
    let world = Arc::new(world());
    let (_, token) = world.source(AlertSourceKind::Alertmanager, None).await;
    let body = notification(
        "firing",
        vec![member("web-1", "warning", "2026-10-27T08:00:00Z")],
    );

    let sends = (0..8).map(|_| {
        let world = world.clone();
        let token = token.clone();
        let body = body.clone();
        tokio::spawn(async move { world.send(&token, &body).await })
    });

    let mut outcomes = Vec::new();

    for send in sends {
        outcomes.push(outcome(&send.await.unwrap()).to_string());
    }

    assert_eq!(world.all().len(), 1);
    assert_eq!(
        outcomes.iter().filter(|o| o.as_str() == "opened").count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| o.as_str() == "unchanged")
            .count(),
        7
    );
}

#[tokio::test]
async fn alerts_received_by_another_cluster_open_incidents_on_the_home_cluster() {
    let world = world();
    *world.cluster.0.lock().unwrap() = "cluster-b".into();

    let mut home_a = write("Prometheus", None);
    home_a.home_cluster = Some("cluster-a".into());

    let (source, token) = world
        .sources
        .create(
            &org(),
            &Id::from_string("user-1"),
            AlertSourceKind::Api,
            home_a,
        )
        .await
        .unwrap();

    // 接收集群不是本集群：原样转过去，把它的答复回给发送方。
    *world.forwarder.reply.lock().unwrap() = Some(IntakeReply {
        status: 200,
        body: json!({"alerts": [{"outcome": "opened"}]}),
    });

    let trigger = json!({"dedup_key": "k", "summary": "Disk full"});
    let reply = world.send(&token, &trigger).await;

    assert_eq!(outcome(&reply), "opened");
    assert!(world.all().is_empty());
    assert_eq!(world.last_request().status, IntakeStatus::Forwarded);
    assert_eq!(
        world.forwarder.sent.lock().unwrap()[0],
        (
            "cluster-a".to_string(),
            "cluster-b".to_string(),
            source.id.0.clone()
        )
    );

    // 接收集群连不上：回 503，发送方会重试。
    *world.forwarder.reply.lock().unwrap() = None;
    assert_eq!(world.send(&token, &trigger).await.status, 503);

    // 转来的请求只在接收集群上处理，不再往别处转。
    let body = serde_json::to_vec(&trigger).unwrap();
    let elsewhere = world
        .sources
        .intake_forwarded(&org(), &source.id, "cluster-c", None, &body)
        .await;
    assert_eq!(elsewhere.status, 503);

    *world.cluster.0.lock().unwrap() = "cluster-a".into();
    let at_home = world
        .sources
        .intake_forwarded(&org(), &source.id, "cluster-b", None, &body)
        .await;

    assert_eq!(outcome(&at_home), "opened");
    assert_eq!(world.all().len(), 1);
    assert_eq!(world.last_request().via_cluster, "cluster-b");

    // 不是本集群也不是启用的远端集群的接收集群不收。
    let mut nowhere = write("Prometheus", None);
    nowhere.home_cluster = Some("cluster-z".into());

    assert!(
        world
            .sources
            .create(
                &org(),
                &Id::from_string("user-1"),
                AlertSourceKind::Api,
                nowhere
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn removing_a_source_resolves_what_it_left_open() {
    let world = world();
    let (source, token) = world.source(AlertSourceKind::Api, None).await;

    world
        .send(&token, &json!({"dedup_key": "k", "summary": "Disk full"}))
        .await;

    world
        .sources
        .delete(&org(), &source.id, &Id::from_string("user-2"))
        .await
        .unwrap();

    let incident = world.all().remove(0);
    assert_eq!(incident.status, IncidentStatus::Resolved);
    assert_eq!(incident.resolved_by, Some(Id::from_string("user-2")));
    assert!(world.sources.list(&org()).await.unwrap().is_empty());

    // 联邦里别的集群删了它：本集群的活跃事件也解决。
    world.sources.apply_replica(source.clone()).await.unwrap();
    world
        .send(
            &token,
            &json!({"dedup_key": "j", "summary": "Queue backlog"}),
        )
        .await;

    world
        .sources
        .remove_replica(&org(), &source.id)
        .await
        .unwrap();

    assert!(
        world
            .all()
            .iter()
            .all(|incident| incident.status == IncidentStatus::Resolved)
    );
    assert!(world.sources.list(&org()).await.unwrap().is_empty());
}
