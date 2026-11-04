// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警来源：管理来源，以及接入外部系统发来的告警（见 [`intake`]）。
//!
//! 来源随联邦同步（路由里调 `emit_cud`），事件只在来源的接收集群上开：别的集群收到它的告警，经
//! 集群之间的 gRPC 转给接收集群（[`AlertForwarder`]），所以一条外部告警不管从哪个地址进来都只
//! 对应一个事件。

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use serde::Serialize;

use crate::{
    app::alerting::{AlertingService, RuleEvaluator},
    domain::{
        alerting::{
            escalation::DEFAULT_POLICY_ID,
            incident::{Incident, Severity},
            repositories::{EscalationPolicyRepository, IncidentRepository},
            source::{
                AlertSource, AlertSourceKind, AlertSourceRepository, EmailRules, ExternalAction,
                ExternalAlert, IntakeRequest, IntakeRequestRepository, MailboxRepository,
                SourceIncident, SourceIncidentRepository,
            },
            timeline::ActionVia,
        },
        iam::InstanceSettingsRepository,
    },
    infra::cluster::RemoteClustersRepository,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod formats;
mod forward;
mod intake;
mod mailbox;
mod token;

pub use forward::{AlertForwarder, ForwardedRequest};
pub use intake::IntakeReply;
pub use mailbox::{
    FetchedMail, MAX_MAIL_BYTES, MailBatch, MailboxClient, MailboxPoller, MailboxStatus,
    MailboxWrite,
};

/// 新建或修改来源时写的部分。
#[derive(Debug, Clone)]
pub struct SourceWrite {
    pub name: String,
    pub enabled: bool,
    pub escalation_policy_id: Option<Id>,
    pub severity: Severity,
    pub labels: BTreeMap<String, String>,
    /// 接收集群；`None` 时新建的来源用本集群，修改时不变。
    pub home_cluster: Option<String>,
    /// 邮件来源怎么读一封邮件；`None` 时新建的用默认的，修改时不变。
    pub email: Option<EmailRules>,
}

/// 接收集群可以选的一个联邦集群。
#[derive(Debug, Clone, Serialize)]
pub struct ClusterChoice {
    pub id: String,
    pub name: String,
    /// 就是本集群。
    pub local: bool,
}

/// 本集群在联邦里的 ID。
#[async_trait]
pub trait FederationCluster: Send + Sync {
    /// 没开联邦时为空。
    async fn cluster_id(&self) -> Result<String>;
}

/// 从实例设置读本集群的 ID：填了集群 ID 就是开了联邦。
pub struct InstanceSettingsCluster(pub Arc<dyn InstanceSettingsRepository>);

#[async_trait]
impl FederationCluster for InstanceSettingsCluster {
    async fn cluster_id(&self) -> Result<String> {
        let settings = self.0.get().await?;

        Ok(if settings.federation_enabled() {
            settings.federation_cluster_id.trim().to_string()
        } else {
            String::new()
        })
    }
}

/// 一个外部事件在来源这一侧的样子：来源（已删除时为空）和来源侧的状态。
#[derive(Debug, Clone)]
pub struct IncidentSource {
    pub source: Option<AlertSource>,
    pub state: SourceIncident,
}

pub struct AlertSources {
    sources: Arc<dyn AlertSourceRepository>,
    requests: Arc<dyn IntakeRequestRepository>,
    source_incidents: Arc<dyn SourceIncidentRepository>,
    incidents: Arc<dyn IncidentRepository>,
    escalations: Arc<dyn EscalationPolicyRepository>,
    evaluator: Arc<RuleEvaluator>,
    alerting: Arc<AlertingService>,
    cluster: Arc<dyn FederationCluster>,
    remote_clusters: Arc<dyn RemoteClustersRepository>,
    forwarder: Option<Arc<dyn AlertForwarder>>,
    mailboxes: Arc<dyn MailboxRepository>,
    mail_client: Option<Arc<dyn MailboxClient>>,
}

/// 构造 [`AlertSources`] 用到的仓库与服务。
pub struct AlertSourcesParts {
    pub sources: Arc<dyn AlertSourceRepository>,
    pub requests: Arc<dyn IntakeRequestRepository>,
    pub source_incidents: Arc<dyn SourceIncidentRepository>,
    pub incidents: Arc<dyn IncidentRepository>,
    pub escalations: Arc<dyn EscalationPolicyRepository>,
    pub evaluator: Arc<RuleEvaluator>,
    pub alerting: Arc<AlertingService>,
    pub cluster: Arc<dyn FederationCluster>,
    pub remote_clusters: Arc<dyn RemoteClustersRepository>,
    pub mailboxes: Arc<dyn MailboxRepository>,
}

impl AlertSources {
    pub fn new(parts: AlertSourcesParts) -> Self {
        Self {
            sources: parts.sources,
            requests: parts.requests,
            source_incidents: parts.source_incidents,
            incidents: parts.incidents,
            escalations: parts.escalations,
            evaluator: parts.evaluator,
            alerting: parts.alerting,
            cluster: parts.cluster,
            remote_clusters: parts.remote_clusters,
            forwarder: None,
            mailboxes: parts.mailboxes,
            mail_client: None,
        }
    }

    /// 接上读邮箱的那一层：试连邮箱要它。
    pub fn with_mail_client(mut self, client: Arc<dyn MailboxClient>) -> Self {
        self.mail_client = Some(client);
        self
    }

    /// 联邦开着时，把接收集群不是本集群的来源的请求经 `forwarder` 转过去。
    pub fn with_forwarder(mut self, forwarder: Arc<dyn AlertForwarder>) -> Self {
        self.forwarder = Some(forwarder);
        self
    }

    /// 本集群在联邦里的 ID；没开联邦时为空。
    pub async fn local_cluster(&self) -> Result<String> {
        self.cluster.cluster_id().await
    }

    /// 接收集群可以选哪些：本集群与启用的远端集群。没开联邦时为空。
    pub async fn clusters(&self) -> Result<Vec<ClusterChoice>> {
        let local = self.local_cluster().await?;

        if local.is_empty() {
            return Ok(Vec::new());
        }

        let mut choices = vec![ClusterChoice {
            id: local.clone(),
            name: local.clone(),
            local: true,
        }];

        for cluster in self.remote_clusters.list_enabled().await? {
            if cluster.id.0 != local {
                choices.push(ClusterChoice {
                    id: cluster.id.0,
                    name: cluster.name,
                    local: false,
                });
            }
        }

        Ok(choices)
    }

    pub async fn list(&self, org_id: &Id) -> Result<Vec<AlertSource>> {
        self.sources.list(org_id).await
    }

    /// 本组织的一个来源；别的组织的来源当作不存在。
    pub async fn get(&self, org_id: &Id, id: &Id) -> Result<AlertSource> {
        let source = self.sources.get(id).await?;

        if &source.org_id != org_id {
            return Err(Error::not_found("alert source not found"));
        }

        Ok(source)
    }

    /// 新建一个来源，返回它和它的 Token（Token 只在这时出现一次）。
    pub async fn create(
        &self,
        org_id: &Id,
        created_by: &Id,
        kind: AlertSourceKind,
        write: SourceWrite,
    ) -> Result<(AlertSource, String)> {
        let issued = token::issue();
        let now = TimestampMicros::now();

        let home_cluster = match write.home_cluster.clone() {
            Some(home) => home,
            None => self.local_cluster().await?,
        };

        let source = AlertSource {
            id: Id::new(),
            org_id: org_id.clone(),
            name: write.name.trim().to_string(),
            kind,
            enabled: write.enabled,
            escalation_policy_id: write.escalation_policy_id,
            severity: write.severity,
            labels: write.labels,
            token_prefix: issued.prefix,
            token_hash: issued.hash,
            home_cluster: home_cluster.trim().to_string(),
            created_by: Some(created_by.clone()),
            created_at: now,
            updated_at: now,
            last_intake_at: None,
            email: write.email.unwrap_or_default(),
        };

        self.check(&source).await?;
        let source = self.sources.create(source).await?;
        Ok((source, issued.token))
    }

    /// 改名称、启停、升级策略、默认严重度、附加标签与接收集群；类型和 Token 不变。
    pub async fn update(&self, org_id: &Id, id: &Id, write: SourceWrite) -> Result<AlertSource> {
        let mut source = self.get(org_id, id).await?;

        source.name = write.name.trim().to_string();
        source.enabled = write.enabled;
        source.escalation_policy_id = write.escalation_policy_id;
        source.severity = write.severity;
        source.labels = write.labels;

        if let Some(home) = write.home_cluster {
            source.home_cluster = home.trim().to_string();
        }

        if let Some(email) = write.email {
            source.email = email;
        }

        source.updated_at = TimestampMicros::now();
        self.check(&source).await?;
        self.sources.update(source).await
    }

    /// 换一个新 Token，旧的立即失效（联邦的其他集群在同步到以后失效）。
    pub async fn rotate_token(&self, org_id: &Id, id: &Id) -> Result<(AlertSource, String)> {
        let mut source = self.get(org_id, id).await?;
        let issued = token::issue();

        source.token_prefix = issued.prefix;
        source.token_hash = issued.hash;
        source.updated_at = TimestampMicros::now();

        let source = self.sources.update(source).await?;
        Ok((source, issued.token))
    }

    /// 删除来源，并以 `by` 的名义解决它所有活跃事件：来源没了，它们再也收不到恢复。
    pub async fn delete(&self, org_id: &Id, id: &Id, by: &Id) -> Result<AlertSource> {
        let source = self.get(org_id, id).await?;
        let now = TimestampMicros::now();

        for incident in self.incidents.list_active_for_rule(org_id, id).await? {
            if incident.external_source_id() == Some(id) {
                self.alerting
                    .resolve(&incident.id, by.clone(), now, ActionVia::Web)
                    .await?;
            }
        }

        self.sources.delete(id).await?;
        Ok(source)
    }

    /// 联邦里别的集群建了或改了这个来源：照写到本集群（本集群的 `last_intake_at` 不动）。
    pub async fn apply_replica(&self, source: AlertSource) -> Result<()> {
        match self.sources.get(&source.id).await {
            Ok(_) => self.sources.update(source).await.map(drop),
            Err(Error::NotFound(_)) => self.sources.create(source).await.map(drop),
            Err(error) => Err(error),
        }
    }

    /// 联邦里别的集群删了这个来源：解决它在本集群的活跃事件（这里是它的接收集群时会有），再删掉。
    pub async fn remove_replica(&self, org_id: &Id, id: &Id) -> Result<()> {
        let source = match self.get(org_id, id).await {
            Ok(source) => source,
            Err(Error::NotFound(_)) => return Ok(()),
            Err(error) => return Err(error),
        };

        let now = TimestampMicros::now();
        let resolve = ExternalAlert {
            action: ExternalAction::Resolve,
            dedup_key: String::new(),
            summary: String::new(),
            severity: None,
            labels: BTreeMap::new(),
            annotations: BTreeMap::new(),
            occurred_at: None,
            no_data: false,
            members: Vec::new(),
            priority: None,
            urgency: None,
        };

        for incident in self.incidents.list_active_for_rule(org_id, id).await? {
            if incident.external_source_id() == Some(id) {
                self.evaluator
                    .settle_external(&source, &resolve, Some(incident), now)
                    .await?;
            }
        }

        self.sources.delete(id).await
    }

    /// 一个来源最近收到的请求，新的在前。
    pub async fn requests(&self, org_id: &Id, id: &Id, limit: u32) -> Result<Vec<IntakeRequest>> {
        let source = self.get(org_id, id).await?;
        self.requests.list(org_id, &source.id, limit).await
    }

    /// 外部事件在来源这一侧的样子；不是外部来源开的事件返回 `None`。
    pub async fn incident_source(&self, incident: &Incident) -> Result<Option<IncidentSource>> {
        let Some(source_id) = incident.external_source_id() else {
            return Ok(None);
        };

        let Some(state) = self.source_incidents.get(&incident.id).await? else {
            return Ok(None);
        };

        let source = match self.sources.get(source_id).await {
            Ok(source) if source.org_id == incident.org_id => Some(source),
            Ok(_) | Err(Error::NotFound(_)) => None,
            Err(error) => return Err(error),
        };

        Ok(Some(IncidentSource { source, state }))
    }

    /// 名称、标签、升级策略、接收集群都要对。
    async fn check(&self, source: &AlertSource) -> Result<()> {
        source.validate()?;

        if let Some(policy_id) = &source.escalation_policy_id
            && policy_id.0 != DEFAULT_POLICY_ID
        {
            let policies = self.escalations.list(&source.org_id).await?;

            if !policies.iter().any(|policy| &policy.id == policy_id) {
                return Err(Error::invalid(format!(
                    "escalation_policy_id `{policy_id}` does not match an escalation policy in this organization"
                )));
            }
        }

        // 没开联邦时（可选的集群为空）接收集群不起作用，不必校验。
        let home = source.home_cluster.as_str();
        let clusters = self.clusters().await?;

        if !home.is_empty()
            && !clusters.is_empty()
            && !clusters.iter().any(|choice| choice.id == home)
        {
            return Err(Error::invalid(format!(
                "home_cluster `{home}` is neither this cluster nor an enabled remote cluster"
            )));
        }

        Ok(())
    }
}
