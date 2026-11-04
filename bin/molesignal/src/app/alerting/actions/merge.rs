// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 合并：几个事件并到一个主事件下。
//!
//! 并入的事件照旧算进行中，它的规则或来源因此不会再开新事件；但它不再升级、不再通知，列表里也不
//! 单独出现。它自己恢复时就解决，并在主事件的时间线上记一笔；主事件解决时一并解决。合并后不能拆开。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use super::{Actor, IncidentActions, check_actionable};
use crate::{
    app::alerting::AlertIncidentLifecycleSink,
    domain::alerting::{
        handling::MAX_MERGE_SOURCES,
        incident::{Incident, IncidentStatus},
        repositories::IncidentRepository,
        timeline::{IncidentTimelineRepository, TimelineEntry, TimelineKind},
    },
    shared::{Error, Result, ids::Id},
};

impl IncidentActions {
    /// 把 `source_ids` 并入 `target_id`，返回主事件。并入的事件自己并入过的，一起改挂到主事件下。
    pub async fn merge(
        &self,
        actor: &Actor,
        target_id: &Id,
        source_ids: &[Id],
    ) -> Result<Incident> {
        let mut sources: Vec<Id> = Vec::new();

        for id in source_ids {
            if id != target_id && !sources.contains(id) {
                sources.push(id.clone());
            }
        }

        if sources.is_empty() {
            return Err(Error::invalid(
                "name at least one other incident to merge in",
            ));
        }

        if sources.len() > MAX_MERGE_SOURCES {
            return Err(Error::invalid(format!(
                "at most {MAX_MERGE_SOURCES} incidents can be merged at a time"
            )));
        }

        let target = self.get(actor, target_id).await?;
        check_actionable(&target)?;

        for id in &sources {
            check_actionable(&self.get(actor, id).await?)?;
        }

        let mut merged = Vec::new();

        for id in &sources {
            let incident = self.merge_one(actor, id, target_id).await?;

            for child in self.incidents.list_merged_into(&actor.org_id, id).await? {
                self.merge_one(actor, &child.id, target_id).await?;
            }

            merged.push(json!({ "id": incident.id, "summary": incident.summary }));
        }

        self.record(
            actor,
            &target,
            TimelineKind::MergedIn,
            json!({ "incidents": merged }),
        )
        .await;

        self.get(actor, target_id).await
    }

    /// 并入一个事件：标上主事件，取消暂缓，停下它还没发完的通知与后续步骤。
    async fn merge_one(&self, actor: &Actor, id: &Id, target_id: &Id) -> Result<Incident> {
        let incident = self
            .modify(actor, id, |incident| {
                if !incident.is_active() {
                    return Ok(false);
                }

                incident.handling.merged_into = Some(target_id.clone());
                incident.handling.clear_snooze();
                Ok(true)
            })
            .await?;

        if let Some(engine) = &self.notify_engine
            && let Err(error) = engine
                .acknowledge_alert(&incident.org_id, &incident.id, actor.at)
                .await
        {
            tracing::warn!(incident_id = %incident.id, %error, "notifications of a merged incident could not be stopped");
        }

        self.record(
            actor,
            &incident,
            TimelineKind::Merged,
            json!({ "into": target_id }),
        )
        .await;

        Ok(incident)
    }
}

/// 组合规则开了事件后，把子规则进行中的事件并进来。
#[async_trait]
pub trait IncidentMerger: Send + Sync {
    async fn merge_children(&self, org_id: &Id, target_id: &Id, children: &[Id]) -> Result<()>;
}

#[async_trait]
impl IncidentMerger for IncidentActions {
    async fn merge_children(&self, org_id: &Id, target_id: &Id, children: &[Id]) -> Result<()> {
        for chunk in children.chunks(MAX_MERGE_SOURCES) {
            self.merge(&Actor::system(org_id), target_id, chunk).await?;
        }

        Ok(())
    }
}

/// 并入的事件自己恢复了（规则或来源报了恢复，没有解决人）：在主事件的时间线上记一笔。主事件解决时
/// 连带解决的有解决人，不记。
pub struct MergedRecoverySink {
    incidents: Arc<dyn IncidentRepository>,
    timeline: Arc<dyn IncidentTimelineRepository>,
}

impl MergedRecoverySink {
    pub fn new(
        incidents: Arc<dyn IncidentRepository>,
        timeline: Arc<dyn IncidentTimelineRepository>,
    ) -> Self {
        Self {
            incidents,
            timeline,
        }
    }
}

#[async_trait]
impl AlertIncidentLifecycleSink for MergedRecoverySink {
    async fn on_incident_changed(&self, incident: &Incident) -> Result<()> {
        let Some(target_id) = &incident.handling.merged_into else {
            return Ok(());
        };

        if incident.status != IncidentStatus::Resolved || incident.resolved_by.is_some() {
            return Ok(());
        }

        let target = self.incidents.get(target_id).await?;

        if !target.is_active() {
            return Ok(());
        }

        let entry = TimelineEntry::new(
            &target.org_id,
            &target.id,
            incident.resolved_at.unwrap_or(incident.created_at),
            TimelineKind::MergedRecovered,
            None,
            json!({ "incidentId": incident.id, "summary": incident.summary }),
        );

        self.timeline.append(entry).await?;
        Ok(())
    }
}
