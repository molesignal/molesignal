// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 开事件之前交给告警流水线（[`crate::app::alerting::event_pipelines`]），事件写库后把流水线改了什么
//! 记进它的时间线，以及来源重新报严重度时经流水线再定一次。没有接上流水线的评估器原样放行。

use super::RuleEvaluator;
use crate::{
    app::alerting::event_pipelines::{AlertOrigin, PipelineTrace, Processed},
    domain::{
        alerting::{
            incident::{Incident, Severity},
            timeline::{TimelineEntry, TimelineKind},
        },
        pipeline::alert::AlertEventSource,
    },
};

impl RuleEvaluator {
    /// 交给告警流水线：改过的事件，或丢弃的原因。规则要开的事件被丢掉时记下规则，这一轮写回规则
    /// 状态时用（见 `record_evaluation`）。
    pub(super) async fn processed(
        &self,
        incident: Incident,
        source: AlertEventSource,
        rule_name: &str,
    ) -> Processed {
        let Some(pipelines) = &self.alert_pipelines else {
            return Processed::Kept(Box::new(incident), Vec::new());
        };

        let rule_id = incident.rule_id.clone();

        let processed = pipelines
            .process(incident, AlertOrigin { source, rule_name })
            .await;

        // 外部来源和拨测的事件不属于规则，没有规则状态可写。
        if matches!(processed, Processed::Dropped(_))
            && matches!(source, AlertEventSource::Rule | AlertEventSource::NoData)
        {
            self.dropped.lock().insert(rule_id);
        }

        processed
    }

    /// 流水线对刚写库的事件做了什么，记进它的时间线；记不下来不影响事件。时刻取事件开出来的那一刻：
    /// 它们发生在写库之前。
    pub(super) async fn record_trail(&self, incident: &Incident, trail: Vec<PipelineTrace>) {
        let Some(timeline) = &self.timeline else {
            return;
        };

        for trace in trail {
            let entry = TimelineEntry::new(
                &incident.org_id,
                &incident.id,
                incident.created_at,
                TimelineKind::PipelineProcessed,
                None,
                trace.detail(),
            );

            if let Err(error) = timeline.append(entry).await {
                tracing::warn!(incident_id = %incident.id, %error, "alert pipeline trace not recorded");
            }
        }
    }

    /// 来源重新报出的事件（`incoming`，和开事件时一样构造）经告警流水线之后的严重度：开事件时
    /// 流水线定过的严重度不会被它冲掉。
    pub(super) async fn processed_severity(
        &self,
        incoming: &Incident,
        source: AlertEventSource,
        rule_name: &str,
    ) -> Severity {
        match &self.alert_pipelines {
            Some(pipelines) => {
                pipelines
                    .severity(incoming, AlertOrigin { source, rule_name })
                    .await
            }
            None => incoming.severity,
        }
    }
}
