// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 工单跟着事件走：新事件按连接器的自动规则建单，确认、解决和备注写成评论，解决时流转。都在事件写入
//! 之外另起任务做，出错只记日志。

use std::sync::Arc;

use async_trait::async_trait;

use super::{TicketService, draft::text};
use crate::{
    app::alerting::AlertIncidentLifecycleSink,
    domain::{
        alerting::{
            incident::{Incident, IncidentStatus},
            ticket::{IncidentTicket, TicketConnector},
        },
        i18n_language::LanguagePack,
    },
    shared::Result,
};

impl TicketService {
    /// 事件变了（生命周期里另起任务调）：新事件按连接器的自动规则建单；确认、解决写成评论，解决时
    /// 流转。出错只记日志。
    pub async fn on_incident(&self, incident: Incident) {
        if let Err(error) = self.follow(&incident).await {
            tracing::warn!(incident_id = %incident.id, %error, "ticket follow-up failed");
        }
    }

    async fn follow(&self, incident: &Incident) -> Result<()> {
        if incident.revision == 0
            && incident.status == IncidentStatus::Open
            && incident.handling.merged_into.is_none()
        {
            for listed in self.repository.list_connectors(&incident.org_id).await? {
                if !listed.enabled || !listed.auto.matches(incident) {
                    continue;
                }

                if let Err(error) = self
                    .create_with(&incident.org_id, &listed.id, incident, None)
                    .await
                {
                    tracing::warn!(incident_id = %incident.id, connector_id = %listed.id, %error, "automatic ticket failed");
                }
            }

            return Ok(());
        }

        if !matches!(
            incident.status,
            IncidentStatus::Acknowledged | IncidentStatus::Resolved | IncidentStatus::Closed
        ) {
            return Ok(());
        }

        let tickets = self
            .repository
            .list_for_incident(&incident.org_id, &incident.id)
            .await?
            .into_iter()
            .filter(|ticket| ticket.action_item_id.is_none())
            .collect::<Vec<_>>();

        if tickets.is_empty() {
            return Ok(());
        }

        let texts = self.texts(&incident.org_id).await;

        for mut ticket in tickets {
            let status = incident.status.as_str();

            if ticket.incident_status == status {
                continue;
            }

            ticket.incident_status = status.to_string();

            let Some(connector) = self
                .repository
                .get_connector(&incident.org_id, &ticket.connector_id)
                .await?
                .filter(|connector| connector.enabled)
            else {
                self.repository.update_ticket(ticket).await?;
                continue;
            };

            if let Err(error) = self.tell(&connector, &ticket, incident, &texts).await {
                ticket.last_error = error.to_string().chars().take(500).collect();
            }

            self.repository.update_ticket(ticket).await?;
        }

        Ok(())
    }

    async fn tell(
        &self,
        connector: &TicketConnector,
        ticket: &IncidentTicket,
        incident: &Incident,
        texts: &LanguagePack,
    ) -> Result<()> {
        let resolved = matches!(
            incident.status,
            IncidentStatus::Resolved | IncidentStatus::Closed
        );
        let comment = match incident.status {
            IncidentStatus::Acknowledged if connector.comments.acknowledged => {
                Some(text(texts, "comment_acknowledged", &[]))
            }
            _ if resolved && connector.comments.resolved => {
                Some(text(texts, "comment_resolved", &[]))
            }
            _ => None,
        };
        let finish = resolved && connector.transition_on_resolve && !ticket.done;

        if comment.is_none() && !finish {
            return Ok(());
        }

        let client = self.client(connector).await?;

        if let Some(comment) = comment {
            client.comment(ticket, &comment).await?;
        }

        if finish {
            client
                .finish(ticket, &text(texts, "comment_resolved", &[]))
                .await?;
        }

        Ok(())
    }

    /// 事件上写了备注：开了「备注写成评论」的连接器上，把它写进工单。出错只记日志。
    pub async fn on_note(&self, incident: &Incident, note: &str) {
        let tickets = match self
            .repository
            .list_for_incident(&incident.org_id, &incident.id)
            .await
        {
            Ok(tickets) => tickets,
            Err(error) => {
                tracing::warn!(incident_id = %incident.id, %error, "tickets of an incident could not be read");
                return;
            }
        };

        if tickets.is_empty() {
            return;
        }

        let texts = self.texts(&incident.org_id).await;
        let comment = text(&texts, "comment_note", &[("text", note)]);

        for ticket in tickets
            .into_iter()
            .filter(|ticket| ticket.action_item_id.is_none())
        {
            let connector = match self
                .repository
                .get_connector(&incident.org_id, &ticket.connector_id)
                .await
            {
                Ok(Some(connector)) if connector.enabled && connector.comments.notes => connector,
                _ => continue,
            };

            let written = match self.client(&connector).await {
                Ok(client) => client.comment(&ticket, &comment).await,
                Err(error) => Err(error),
            };

            if let Err(error) = written {
                tracing::warn!(incident_id = %incident.id, ticket = %ticket.external_key, %error, "a note could not be written to the ticket");
            }
        }
    }
}

/// 事件变了就交给工单：另起任务，不拖慢事件的写入。
pub struct TicketLifecycleSink(pub Arc<TicketService>);

#[async_trait]
impl AlertIncidentLifecycleSink for TicketLifecycleSink {
    async fn on_incident_changed(&self, incident: &Incident) -> Result<()> {
        let service = self.0.clone();
        let incident = incident.clone();

        tokio::spawn(async move { service.on_incident(incident).await });
        Ok(())
    }
}
