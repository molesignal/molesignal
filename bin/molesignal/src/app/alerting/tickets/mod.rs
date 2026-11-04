// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 工单：在 Jira、ServiceNow 或飞书项目里为事件建单，把事件的确认、解决与备注写成评论、解决时流转，定时
//! 同步工单状态。一个事件在一个连接器上只建一张单。写给工单的文字用工作区的默认语言（`alerting.ticket.*`）。

mod draft;
mod follow;
pub mod jira;
pub mod meegle;
pub mod servicenow;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};

pub use self::{draft::TicketDraft, follow::TicketLifecycleSink};
use self::{
    jira::JiraClient,
    meegle::{MeegleClient, PluginToken},
    servicenow::ServiceNowClient,
};
use crate::{
    domain::{
        alerting::{
            incident::Incident,
            repositories::{IncidentRcaRepository, IncidentRepository},
            ticket::{
                IncidentTicket, TICKET_SYNC_INTERVAL_MICROS, TicketConnector, TicketRepository,
                TicketSystem,
            },
            timeline::{IncidentTimelineRepository, TimelineEntry, TimelineKind},
        },
        i18n_language::I18nLanguageRepository,
    },
    infra::persistence::repositories::workspace_preference_defaults::WorkspacePreferenceDefaultsRepository,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 一轮最多同步多少张工单。
const SYNC_BATCH: i64 = 50;

#[derive(Debug, Clone)]
pub struct CreatedTicket {
    pub external_id: String,
    pub external_key: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketState {
    pub status: String,
    pub done: bool,
}

enum Client {
    Jira(JiraClient),
    Meegle(MeegleClient),
    ServiceNow(ServiceNowClient),
}

impl Client {
    async fn create(&self, draft: &TicketDraft) -> Result<CreatedTicket> {
        match self {
            Self::Jira(client) => client.create(draft).await,
            Self::Meegle(client) => client.create(draft).await,
            Self::ServiceNow(client) => client.create(draft).await,
        }
    }

    async fn state(&self, ticket: &IncidentTicket) -> Result<TicketState> {
        match self {
            Self::Jira(client) => client.state(&ticket.external_key).await,
            Self::Meegle(client) => client.state(&ticket.external_id).await,
            Self::ServiceNow(client) => client.state(&ticket.external_id).await,
        }
    }

    async fn comment(&self, ticket: &IncidentTicket, text: &str) -> Result<()> {
        match self {
            Self::Jira(client) => client.comment(&ticket.external_key, text).await,
            Self::Meegle(client) => client.comment(&ticket.external_id, text).await,
            Self::ServiceNow(client) => client.comment(&ticket.external_id, text).await,
        }
    }

    /// `note` 是解决说明：ServiceNow 解决记录时要写。
    async fn finish(&self, ticket: &IncidentTicket, note: &str) -> Result<bool> {
        match self {
            Self::Jira(client) => Ok(client.finish(&ticket.external_key).await?.is_some()),
            Self::Meegle(client) => client.finish(&ticket.external_id).await,
            Self::ServiceNow(client) => {
                client.finish(&ticket.external_id, note).await?;
                Ok(true)
            }
        }
    }
}

pub struct TicketService {
    repository: Arc<dyn TicketRepository>,
    incidents: Arc<dyn IncidentRepository>,
    timeline: Arc<dyn IncidentTimelineRepository>,
    rca: Arc<dyn IncidentRcaRepository>,
    i18n: Arc<dyn I18nLanguageRepository>,
    workspace_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository>,
    external_url: String,
    /// 飞书项目的插件 Token、ServiceNow 的 OAuth 令牌，按连接器缓存。
    tokens: Mutex<HashMap<Id, PluginToken>>,
}

impl TicketService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repository: Arc<dyn TicketRepository>,
        incidents: Arc<dyn IncidentRepository>,
        timeline: Arc<dyn IncidentTimelineRepository>,
        rca: Arc<dyn IncidentRcaRepository>,
        i18n: Arc<dyn I18nLanguageRepository>,
        workspace_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository>,
        external_url: String,
    ) -> Self {
        Self {
            repository,
            incidents,
            timeline,
            rca,
            i18n,
            workspace_defaults,
            external_url: external_url.trim().trim_end_matches('/').to_string(),
            tokens: Mutex::new(HashMap::new()),
        }
    }

    pub fn repository(&self) -> &Arc<dyn TicketRepository> {
        &self.repository
    }

    async fn client(&self, connector: &TicketConnector) -> Result<Client> {
        match connector.system {
            TicketSystem::Jira => Ok(Client::Jira(JiraClient::new(connector).await?)),
            TicketSystem::Meegle => {
                let now_secs = TimestampMicros::now().0 / 1_000_000;
                let cached = self.cached_token(&connector.id);
                let (client, renewed) = MeegleClient::connect(connector, cached, now_secs).await?;

                self.keep_token(&connector.id, renewed);
                Ok(Client::Meegle(client))
            }
            TicketSystem::Servicenow => {
                let now_secs = TimestampMicros::now().0 / 1_000_000;
                let cached = self.cached_token(&connector.id);
                let (client, renewed) =
                    ServiceNowClient::connect(connector, cached, now_secs).await?;

                self.keep_token(&connector.id, renewed);
                Ok(Client::ServiceNow(client))
            }
        }
    }

    fn cached_token(&self, connector_id: &Id) -> Option<PluginToken> {
        self.tokens
            .lock()
            .ok()
            .and_then(|tokens| tokens.get(connector_id).cloned())
    }

    fn keep_token(&self, connector_id: &Id, renewed: Option<PluginToken>) {
        if let (Some(token), Ok(mut tokens)) = (renewed, self.tokens.lock()) {
            tokens.insert(connector_id.clone(), token);
        }
    }

    /// 连接器连得上：凭证、项目或空间、类型都对。返回一句说明（Jira 是登录的人）。
    pub async fn check(&self, connector: &TicketConnector) -> Result<String> {
        match self.client(connector).await? {
            Client::Jira(client) => client.check().await,
            Client::Meegle(client) => client.check().await.map(|()| String::new()),
            Client::ServiceNow(client) => client.check().await,
        }
    }

    /// 在 `connector_id` 上为事件建单；这个连接器上已经有单时返回它。
    pub async fn create_with(
        &self,
        org_id: &Id,
        connector_id: &Id,
        incident: &Incident,
        by: Option<&Id>,
    ) -> Result<IncidentTicket> {
        let connector = self
            .repository
            .get_connector(org_id, connector_id)
            .await?
            .ok_or_else(|| Error::not_found("ticket connector not found"))?;

        self.create(&connector, incident, by).await
    }

    pub async fn create(
        &self,
        connector: &TicketConnector,
        incident: &Incident,
        by: Option<&Id>,
    ) -> Result<IncidentTicket> {
        if !connector.enabled {
            return Err(Error::invalid("this ticket connector is turned off"));
        }

        if let Some(existing) = self
            .repository
            .list_for_incident(&incident.org_id, &incident.id)
            .await?
            .into_iter()
            .find(|ticket| ticket.connector_id == connector.id && ticket.action_item_id.is_none())
        {
            return Ok(existing);
        }

        let draft = self.draft(incident).await;
        let created = self.client(connector).await?.create(&draft).await?;
        let now = TimestampMicros::now();

        let ticket = self
            .repository
            .create_ticket(IncidentTicket {
                id: Id::new(),
                org_id: incident.org_id.clone(),
                incident_id: incident.id.clone(),
                connector_id: connector.id.clone(),
                system: connector.system,
                external_id: created.external_id,
                external_key: created.external_key,
                url: created.url,
                status: String::new(),
                done: false,
                incident_status: incident.status.as_str().to_string(),
                created_by: by.cloned(),
                created_at: now,
                synced_at: now,
                last_error: String::new(),
                action_item_id: None,
            })
            .await?;

        self.record(
            &ticket,
            TimelineKind::TicketCreated,
            by.cloned(),
            json!({
                "system": ticket.system,
                "key": ticket.external_key,
                "url": ticket.url,
                "connector": connector.name,
            }),
        )
        .await;

        Ok(ticket)
    }

    /// 同步到了时间的工单；返回因为工单完结、连接器又开了「工单完结时解决事件」而要解决的事件。
    pub async fn sync_due(&self, now: TimestampMicros) -> Result<Vec<Id>> {
        let due = self
            .repository
            .due_for_sync(
                TimestampMicros(now.0 - TICKET_SYNC_INTERVAL_MICROS),
                SYNC_BATCH,
            )
            .await?;

        let mut connectors: HashMap<Id, Option<TicketConnector>> = HashMap::new();
        let mut resolve = Vec::new();

        for mut ticket in due {
            if !connectors.contains_key(&ticket.connector_id) {
                let connector = self
                    .repository
                    .get_connector(&ticket.org_id, &ticket.connector_id)
                    .await?;
                connectors.insert(ticket.connector_id.clone(), connector);
            }

            let connector = connectors
                .get(&ticket.connector_id)
                .and_then(Option::as_ref)
                .filter(|connector| connector.enabled);

            if let Some(connector) = connector {
                match self.fetch(connector, &ticket).await {
                    Ok(state) => {
                        if self.apply(&mut ticket, connector, state, now).await {
                            resolve.push(ticket.incident_id.clone());
                        }
                    }
                    Err(error) => ticket.last_error = error.to_string().chars().take(500).collect(),
                }
            }

            ticket.synced_at = now;
            self.repository.update_ticket(ticket).await?;
        }

        Ok(resolve)
    }

    async fn fetch(
        &self,
        connector: &TicketConnector,
        ticket: &IncidentTicket,
    ) -> Result<TicketState> {
        self.client(connector).await?.state(ticket).await
    }

    /// 记下工单的新状态；变了就记进时间线。返回事件要不要因此解决。
    async fn apply(
        &self,
        ticket: &mut IncidentTicket,
        connector: &TicketConnector,
        state: TicketState,
        now: TimestampMicros,
    ) -> bool {
        let changed = state.status != ticket.status || state.done != ticket.done;
        let finished = state.done && !ticket.done;

        ticket.status = state.status;
        ticket.done = state.done;
        ticket.last_error.clear();
        ticket.synced_at = now;

        if changed {
            self.record(
                ticket,
                TimelineKind::TicketUpdated,
                None,
                json!({ "key": ticket.external_key, "status": ticket.status, "done": ticket.done }),
            )
            .await;
        }

        finished && connector.resolve_incident_when_done && ticket.action_item_id.is_none()
    }

    /// Jira 的 Webhook：验签名，按 issue key 找到工单，记下状态。返回要解决的事件。没设 Webhook 密钥
    /// 的连接器不收。
    pub async fn jira_webhook(
        &self,
        connector_id: &Id,
        signature: Option<&str>,
        body: &[u8],
    ) -> Result<Option<Id>> {
        let connector = self
            .repository
            .find_connector(connector_id)
            .await?
            .filter(|connector| connector.system == TicketSystem::Jira && connector.enabled)
            .ok_or_else(|| Error::not_found("ticket connector not found"))?;

        let secret = connector.secret("webhookSecret").ok_or_else(|| {
            Error::forbidden("this connector takes no webhook: set its secret first")
        })?;

        if !signature.is_some_and(|signature| jira::signature_matches(secret, body, signature)) {
            return Err(Error::unauthorized("the webhook signature does not match"));
        }

        let parsed: Value = serde_json::from_slice(body)
            .map_err(|error| Error::invalid(format!("the webhook body is not JSON: {error}")))?;

        let Some((key, state)) = jira::webhook_issue(&parsed) else {
            return Ok(None);
        };

        let Some(mut ticket) = self
            .repository
            .find_by_external(&connector.id, &key)
            .await?
        else {
            return Ok(None);
        };

        let resolve = self
            .apply(&mut ticket, &connector, state, TimestampMicros::now())
            .await;
        let incident_id = ticket.incident_id.clone();
        self.repository.update_ticket(ticket).await?;

        Ok(resolve.then_some(incident_id))
    }

    /// 为复盘的一项改进建一张单：标题是改进项，描述带上事件；不跟着事件写评论、流转。
    pub async fn create_for_action(
        &self,
        connector: &TicketConnector,
        incident: &Incident,
        action_item_id: &Id,
        title: &str,
        by: Option<&Id>,
    ) -> Result<IncidentTicket> {
        if !connector.enabled {
            return Err(Error::invalid("this ticket connector is turned off"));
        }

        let mut draft = self.draft(incident).await;
        draft.summary = title.chars().take(250).collect();
        draft.labels.push("postmortem".to_string());

        let created = self.client(connector).await?.create(&draft).await?;
        let now = TimestampMicros::now();

        let ticket = self
            .repository
            .create_ticket(IncidentTicket {
                id: Id::new(),
                org_id: incident.org_id.clone(),
                incident_id: incident.id.clone(),
                connector_id: connector.id.clone(),
                system: connector.system,
                external_id: created.external_id,
                external_key: created.external_key,
                url: created.url,
                status: String::new(),
                done: false,
                incident_status: incident.status.as_str().to_string(),
                created_by: by.cloned(),
                created_at: now,
                synced_at: now,
                last_error: String::new(),
                action_item_id: Some(action_item_id.clone()),
            })
            .await?;

        Ok(ticket)
    }

    pub fn incident_url(&self, incident_id: &Id) -> String {
        format!("{}/alerts/incidents/{incident_id}", self.external_url)
    }

    async fn record(
        &self,
        ticket: &IncidentTicket,
        kind: TimelineKind,
        actor: Option<Id>,
        detail: Value,
    ) {
        let entry = TimelineEntry::new(
            &ticket.org_id,
            &ticket.incident_id,
            TimestampMicros::now(),
            kind,
            actor,
            detail,
        );

        if let Err(error) = self.timeline.append(entry).await {
            tracing::warn!(incident_id = %ticket.incident_id, %error, "ticket timeline entry could not be written");
        }
    }

    /// 读出一个事件，看它是不是本组织的。
    pub async fn incident(&self, org_id: &Id, id: &Id) -> Result<Incident> {
        let incident = self.incidents.get(id).await?;

        if &incident.org_id != org_id {
            return Err(Error::not_found("incident not found"));
        }

        Ok(incident)
    }
}
