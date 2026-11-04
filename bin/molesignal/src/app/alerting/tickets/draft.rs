// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 写给工单的内容：一张单的标题与描述，和工作区默认语言的文字（`alerting.ticket.*`）。

use super::TicketService;
use crate::{
    domain::{
        alerting::incident::Incident,
        i18n_language::{FALLBACK_LANGUAGE_CODE, LanguagePack},
    },
    shared::ids::Id,
};

/// 根因摘要最多写多少字进工单。
const MAX_ROOT_CAUSE_CHARS: usize = 1_000;

/// 要建的一张单。
#[derive(Debug, Clone)]
pub struct TicketDraft {
    pub summary: String,
    /// 描述的各段。
    pub lines: Vec<String>,
    /// 事件的地址。
    pub link: String,
    /// 严重度（映射优先级用）。
    pub severity: String,
    /// 事件的优先级 `P1`…`P5`：映射里有它时先按它，没有再按严重度。
    pub priority: Option<String>,
    pub labels: Vec<String>,
    /// 事件的 ID，写进工单方便对回来。
    pub incident_id: String,
}

impl TicketDraft {
    /// 工单的优先级：映射里有事件的优先级（`P1`…`P5`）时按它，没有再按严重度。
    pub fn mapped_priority<'a>(
        &self,
        priorities: &'a std::collections::BTreeMap<String, String>,
    ) -> Option<&'a String> {
        self.priority
            .as_ref()
            .and_then(|priority| priorities.get(priority))
            .or_else(|| priorities.get(&self.severity))
    }
}

impl TicketService {
    /// 一张单的标题与描述：事件、严重度与状态、服务、标签、触发查询、根因摘要和事件的地址。
    pub(super) async fn draft(&self, incident: &Incident) -> TicketDraft {
        let texts = self.texts(&incident.org_id).await;
        let mut lines = vec![text(&texts, "heading", &[("summary", &incident.summary)])];

        lines.push(text(
            &texts,
            "severity",
            &[
                ("severity", incident.severity.as_str()),
                ("status", incident.status.as_str()),
            ],
        ));

        if !incident.affected_services.is_empty() {
            lines.push(text(
                &texts,
                "services",
                &[("services", &incident.affected_services.join(", "))],
            ));
        }

        if !incident.labels.is_empty() {
            let labels = incident
                .labels
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(text(&texts, "labels", &[("labels", &labels)]));
        }

        if let Some(query) = &incident.triggering_query {
            lines.push(text(&texts, "query", &[("query", &query.statement)]));
        }

        if let Ok(Some(rca)) = self.rca.get(&incident.id).await {
            let summary = rca
                .summary
                .chars()
                .take(MAX_ROOT_CAUSE_CHARS)
                .collect::<String>();
            lines.push(text(&texts, "root_cause", &[("summary", &summary)]));
        }

        TicketDraft {
            summary: format!("[{}] {}", incident.severity.as_str(), incident.summary),
            lines,
            link: self.incident_url(&incident.id),
            severity: incident.severity.as_str().to_string(),
            priority: incident
                .handling
                .priority
                .map(|priority| priority.as_str().to_string()),
            labels: vec!["molesignal".to_string()],
            incident_id: incident.id.to_string(),
        }
    }

    pub(super) async fn texts(&self, org_id: &Id) -> LanguagePack {
        let language = self
            .workspace_defaults
            .get(org_id)
            .await
            .map(|defaults| defaults.language)
            .unwrap_or_else(|_| FALLBACK_LANGUAGE_CODE.to_string());

        self.i18n
            .load_pack("alerting.ticket.", &language)
            .await
            .unwrap_or_default()
    }
}

/// `alerting.ticket.<key>`，填上 `fills`；缺了这一条时写出键名。
pub(super) fn text(pack: &LanguagePack, key: &str, fills: &[(&str, &str)]) -> String {
    let key = format!("alerting.ticket.{key}");

    pack.render(&key, fills).unwrap_or(key)
}
