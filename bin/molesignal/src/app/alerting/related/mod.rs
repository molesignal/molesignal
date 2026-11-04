// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 和一个事件有关的别的东西：过去像它的事件，和它触发前后的变更。

mod changes;
mod similar;

use std::{collections::HashMap, sync::Arc};

pub use changes::{
    AFTER, BEFORE, IncidentChange, IncidentChanges, incident_services, select_changes,
};
pub use similar::{SimilarIncident, first_sentence, score, trigram_similarity};

use crate::{
    domain::{
        alerting::{
            incident::Incident,
            repositories::{AlertRuleRepository, IncidentRcaRepository, IncidentRepository},
            timeline::{IncidentTimelineRepository, TimelineKind},
        },
        apm::{ApmCatalogRepository, DeploymentQuery},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 一次最多读多少条上报的变更。
const MOST_CHANGES_READ: usize = 500;

pub struct RelatedIncidents {
    incidents: Arc<dyn IncidentRepository>,
    timeline: Arc<dyn IncidentTimelineRepository>,
    rca: Arc<dyn IncidentRcaRepository>,
    rules: Arc<dyn AlertRuleRepository>,
    catalog: Arc<dyn ApmCatalogRepository>,
}

impl RelatedIncidents {
    pub fn new(
        incidents: Arc<dyn IncidentRepository>,
        timeline: Arc<dyn IncidentTimelineRepository>,
        rca: Arc<dyn IncidentRcaRepository>,
        rules: Arc<dyn AlertRuleRepository>,
        catalog: Arc<dyn ApmCatalogRepository>,
    ) -> Self {
        Self {
            incidents,
            timeline,
            rca,
            rules,
            catalog,
        }
    }

    /// 近 90 天里最像 `incident` 的几个事件，带上备注数和根因的第一句。它自己、并入它的、它并入的
    /// 不算。
    pub async fn similar(&self, incident: &Incident) -> Result<Vec<SimilarIncident>> {
        let since = TimestampMicros(incident.created_at.0 - similar::LOOKBACK_MICROS);

        let mut scored: Vec<(f64, Vec<&'static str>, Incident)> = self
            .incidents
            .list_since(&incident.org_id, since)
            .await?
            .into_iter()
            .filter(|other| {
                other.id != incident.id
                    && other.handling.merged_into.as_ref() != Some(&incident.id)
                    && incident.handling.merged_into.as_ref() != Some(&other.id)
            })
            .filter_map(|other| {
                let (points, reasons) = score(incident, &other);
                (points >= similar::MIN_SCORE).then_some((points, reasons, other))
            })
            .collect();

        scored.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then_with(|| b.2.created_at.0.cmp(&a.2.created_at.0))
        });
        scored.truncate(similar::MOST_SIMILAR);

        let ids: Vec<Id> = scored
            .iter()
            .map(|(_, _, other)| other.id.clone())
            .collect();
        let mut notes: HashMap<Id, u32> = HashMap::new();

        for entry in self
            .timeline
            .list_for_incidents(&incident.org_id, &ids, &[TimelineKind::Note])
            .await?
        {
            *notes.entry(entry.incident_id).or_insert(0) += 1;
        }

        let mut similar = Vec::with_capacity(scored.len());

        for (points, reasons, other) in scored {
            let root_cause = self
                .rca
                .get(&other.id)
                .await?
                .and_then(|rca| first_sentence(&rca.summary));

            similar.push(SimilarIncident {
                notes: notes.get(&other.id).copied().unwrap_or(0),
                id: other.id,
                summary: other.summary,
                severity: other.severity,
                status: other.status,
                created_at: other.created_at,
                resolved_at: other.resolved_at,
                resolved_by: other.resolved_by,
                score: (points * 100.0).round() / 100.0,
                reasons,
                root_cause,
            });
        }

        Ok(similar)
    }

    /// `incident` 触发前后的变更；`downstream` 是它的服务在服务地图上的下游。
    pub async fn changes(
        &self,
        incident: &Incident,
        downstream: &[String],
    ) -> Result<IncidentChanges> {
        let reported = self
            .catalog
            .list_deployments(
                &incident.org_id,
                &DeploymentQuery {
                    namespace: None,
                    service: None,
                    environment: None,
                    from: Some(TimestampMicros(incident.created_at.0 - BEFORE)),
                    to: Some(TimestampMicros(incident.created_at.0 + AFTER)),
                    limit: MOST_CHANGES_READ,
                },
            )
            .await?;

        let rule = self
            .rules
            .get(&incident.rule_id)
            .await
            .ok()
            .filter(|rule| rule.org_id == incident.org_id);

        Ok(select_changes(
            incident,
            &reported,
            downstream,
            rule.as_ref(),
        ))
    }
}
