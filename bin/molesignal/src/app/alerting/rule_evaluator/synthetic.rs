// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 拨测失败开告警事件：监控项的整体状态变成失败，或打开了开关的降级、不稳定时，开一个告警事件，
//! 按监控项的升级策略升级、按通知策略通知；状态回到不用告警时解决它。口径与状态页自动化相同
//! （[`MonitorState::alert_activation`](crate::domain::synthetics::MonitorState::alert_activation)）。
//!
//! 一个监控项同时最多一个状态事件：它的 `rule_id` 是监控项的 id，带标签 `alert.source = synthetic`。
//! 不关联升级策略的监控项照常开事件、照常通知，只是没有可升级的对象。
//!
//! 监控项没有结果时（见 [`NoDataChange`]）另开一个无数据事件，有结果后自动解决；它带
//! `alert.condition = no_data`，指纹与状态事件分开，两者可以同时开着，同规则的无数据事件一样。
//! 监控项暂停或归档后不再出结果，两种事件都在这时解决。

use std::sync::Arc;

use async_trait::async_trait;

use super::{
    RuleEvaluator,
    events::{emit_lifecycle, enqueue_notify_event},
    synthetic_incident::{Opening, synthetic_incident},
};
use crate::{
    app::{
        alerting::event_pipelines::Processed,
        notify::ALERT_TRIGGERED_EVENT,
        synthetics::{NoDataChange, SyntheticTransitionSink},
    },
    domain::{
        alerting::incident::{IncidentStatus, SYNTHETIC_STATE_LABEL, Severity},
        i18n_language::{FALLBACK_LANGUAGE_CODE, I18nLanguageRepository, LanguagePack},
        pipeline::alert::AlertEventSource,
        synthetics::{
            MonitorLifecycle, MonitorRevision, SyntheticMonitor, SyntheticMonitorRepository,
            SyntheticStateTransition,
        },
    },
    infra::persistence::repositories::workspace_preference_defaults::WorkspacePreferenceDefaultsRepository,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 拨测状态的变化交给告警：要告警就开事件或调整严重度，不用告警就解决。只看监控项的整体状态，
/// 单个地点的变化不开事件。
pub struct SyntheticIncidentSink {
    evaluator: Arc<RuleEvaluator>,
    monitors: Arc<dyn SyntheticMonitorRepository>,
    i18n: Arc<dyn I18nLanguageRepository>,
    workspace_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository>,
}

impl SyntheticIncidentSink {
    pub fn new(
        evaluator: Arc<RuleEvaluator>,
        monitors: Arc<dyn SyntheticMonitorRepository>,
        i18n: Arc<dyn I18nLanguageRepository>,
        workspace_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository>,
    ) -> Self {
        Self {
            evaluator,
            monitors,
            i18n,
            workspace_defaults,
        }
    }

    /// 无数据事件的正文，用工作区的默认语言；语言表里没有这些条目时不写。
    async fn no_data_text(&self, change: &NoDataChange) -> Option<String> {
        let language = self
            .workspace_defaults
            .get(&change.monitor.organization_id)
            .await
            .map(|defaults| defaults.language)
            .unwrap_or_else(|_| FALLBACK_LANGUAGE_CODE.to_string());

        let texts = match self.i18n.load_pack("synthetics.", &language).await {
            Ok(texts) => texts,
            Err(error) => {
                tracing::warn!(%error, "synthetic incident texts could not be read");
                return None;
            }
        };

        no_data_text(
            &texts,
            &change.monitor.name,
            &change.locations,
            change.missed_runs,
            change.skip_reason.as_deref(),
        )
    }
}

#[async_trait]
impl SyntheticTransitionSink for SyntheticIncidentSink {
    async fn on_transition(
        &self,
        transition: &SyntheticStateTransition,
        alert_on_degraded: bool,
        alert_on_flaky: bool,
    ) -> Result<()> {
        if transition.location_id.is_some() {
            return Ok(());
        }

        let Some((active, severity)) = transition
            .current_state
            .alert_activation(alert_on_degraded, alert_on_flaky)
        else {
            return Ok(());
        };

        let organization_id = &transition.organization_id;

        let monitor = self
            .monitors
            .get_monitor(organization_id, &transition.monitor_id)
            .await?;

        let revision = self
            .monitors
            .get_revision(
                organization_id,
                &transition.monitor_id,
                &transition.monitor_revision_id,
            )
            .await?;

        let alert = active.then_some(severity);

        self.evaluator
            .settle_synthetic(
                &monitor,
                &revision,
                transition,
                alert,
                TimestampMicros::now(),
            )
            .await
    }

    async fn on_lifecycle(&self, monitor: &SyntheticMonitor) -> Result<()> {
        if matches!(
            monitor.lifecycle,
            MonitorLifecycle::Paused | MonitorLifecycle::Archived
        ) {
            self.evaluator
                .close_synthetic(monitor, TimestampMicros::now())
                .await?;
        }

        Ok(())
    }

    async fn on_no_data(&self, change: &NoDataChange) -> Result<()> {
        self.evaluator
            .settle_synthetic_no_data(
                change,
                async || self.no_data_text(change).await,
                TimestampMicros::now(),
            )
            .await
    }
}

/// 一个监控项的告警事件的指纹，与规则的指纹分开。
pub fn synthetic_fingerprint(monitor_id: &Id) -> String {
    blake3::hash(format!("synthetic|{monitor_id}").as_bytes())
        .to_hex()
        .to_string()
}

/// 一个监控项的无数据事件的指纹，与它的状态事件分开。
pub fn synthetic_no_data_fingerprint(monitor_id: &Id) -> String {
    blake3::hash(format!("synthetic|no_data|{monitor_id}").as_bytes())
        .to_hex()
        .to_string()
}

/// 「检查 X 没有结果：A、B 连续 N 次没有运行，原因。」原因认不出时不写原因。
fn no_data_text(
    texts: &LanguagePack,
    check: &str,
    locations: &[String],
    missed_runs: u32,
    skip_reason: Option<&str>,
) -> Option<String> {
    let separator = texts.get("synthetics.incident.separator")?;
    let locations = locations.join(separator);
    let runs = missed_runs.to_string();

    let fills = [
        ("check", check),
        ("locations", locations.as_str()),
        ("runs", runs.as_str()),
    ];

    let reason =
        skip_reason.and_then(|reason| texts.get(&format!("synthetics.skip_reason.{reason}")));

    match reason {
        Some(reason) => texts
            .render(
                "synthetics.incident.no_data_reason",
                &[fills.as_slice(), &[("reason", reason)]].concat(),
            )
            .ok(),
        None => texts.render("synthetics.incident.no_data", &fills).ok(),
    }
}

impl RuleEvaluator {
    /// 监控项的整体状态变了：`alert` 是要告警时的严重度，`None` 表示不用再告警。开着的事件跟着改
    /// 严重度与状态标签；已确认的事件保持原样，由人解决。
    pub async fn settle_synthetic(
        &self,
        monitor: &SyntheticMonitor,
        revision: &MonitorRevision,
        transition: &SyntheticStateTransition,
        alert: Option<Severity>,
        now: TimestampMicros,
    ) -> Result<()> {
        let fingerprint = synthetic_fingerprint(&monitor.id);

        let existing = self
            .incidents
            .find_by_fingerprint(&monitor.organization_id, &fingerprint)
            .await?;

        let Some(severity) = alert else {
            if let Some(incident) = existing {
                self.resolve_open(incident, now).await?;
            }

            return Ok(());
        };

        let state = transition.current_state.as_str();

        match existing {
            Some(mut incident) if incident.status == IncidentStatus::Open => {
                let incoming = synthetic_incident(
                    monitor,
                    revision,
                    Opening::State {
                        state: transition.current_state,
                        severity,
                        result_id: transition.result_id.as_ref(),
                    },
                    now,
                );

                // 严重度和开事件时一样经过告警流水线；有人手动改过就照人定的来，状态标签照样跟上。
                let severity = if incident.handling.severity_set_by.is_some() {
                    incident.severity
                } else {
                    self.processed_severity(&incoming, AlertEventSource::Synthetic, &monitor.name)
                        .await
                };

                let same_state = incident
                    .labels
                    .get(SYNTHETIC_STATE_LABEL)
                    .map(String::as_str)
                    == Some(state);

                if same_state && incident.severity == severity {
                    return Ok(());
                }

                incident
                    .labels
                    .insert(SYNTHETIC_STATE_LABEL.into(), state.into());

                self.change_severity(incident, severity, now).await
            }
            // 已确认的事件仍是活跃事件；一个监控项同时只开一个。
            Some(incident) if incident.status == IncidentStatus::Acknowledged => Ok(()),
            _ => {
                let opening = Opening::State {
                    state: transition.current_state,
                    severity,
                    result_id: transition.result_id.as_ref(),
                };

                self.open_synthetic_incident(monitor, revision, opening, now)
                    .await
            }
        }
    }

    /// 监控项有没有结果（见 [`NoDataChange`]）：没有、也没有活跃的无数据事件时开一个 warning 事件，
    /// 正文由 `describe` 写；这一段没有结果期间开过、被人解决了的，这一段不再开。有结果了就解决
    /// 开着的无数据事件，已确认的保持原样，由人解决。
    pub async fn settle_synthetic_no_data(
        &self,
        change: &NoDataChange,
        describe: impl AsyncFnOnce() -> Option<String>,
        now: TimestampMicros,
    ) -> Result<()> {
        let monitor = &change.monitor;

        let existing = self
            .incidents
            .find_by_fingerprint(
                &monitor.organization_id,
                &synthetic_no_data_fingerprint(&monitor.id),
            )
            .await?;

        let active = existing.as_ref().is_some_and(|incident| {
            matches!(
                incident.status,
                IncidentStatus::Open | IncidentStatus::Acknowledged
            )
        });

        if !change.no_data {
            if let Some(incident) = existing.filter(|_| active) {
                self.resolve_open(incident, now).await?;
            }

            return Ok(());
        }

        let resolved_in_this_stretch = existing.as_ref().is_some_and(|incident| {
            incident.resolved_by.is_some() && incident.created_at.0 >= change.since.0
        });

        if active || resolved_in_this_stretch {
            return Ok(());
        }

        let opening = Opening::NoData {
            description: describe().await,
        };

        self.open_synthetic_incident(monitor, &change.revision, opening, now)
            .await
    }

    /// 监控项暂停或归档了，不会再有结果让它的事件恢复：开着或已确认的状态事件、无数据事件都解决。
    pub async fn close_synthetic(
        &self,
        monitor: &SyntheticMonitor,
        now: TimestampMicros,
    ) -> Result<()> {
        let fingerprints = [
            synthetic_fingerprint(&monitor.id),
            synthetic_no_data_fingerprint(&monitor.id),
        ];

        for fingerprint in fingerprints {
            if let Some(incident) = self
                .incidents
                .find_by_fingerprint(&monitor.organization_id, &fingerprint)
                .await?
            {
                self.resolve_active(incident, now).await?;
            }
        }

        Ok(())
    }

    /// 开拨测事件：交给告警流水线，路由后写入。事件的样子见 [`synthetic_incident`]。
    async fn open_synthetic_incident(
        &self,
        monitor: &SyntheticMonitor,
        revision: &MonitorRevision,
        opening: Opening<'_>,
        now: TimestampMicros,
    ) -> Result<()> {
        let incident = synthetic_incident(monitor, revision, opening, now);

        let Processed::Kept(incident, trail) = self
            .processed(incident, AlertEventSource::Synthetic, &monitor.name)
            .await
        else {
            return Ok(());
        };

        let incident = self.routed(*incident).await;

        let incident = match self.incidents.create(incident).await {
            Ok(incident) => incident,
            // 两个节点同时处理到这次变化，另一个已经开了。
            Err(Error::Conflict(_)) => return Ok(()),
            Err(error) => return Err(error),
        };

        self.record_trail(&incident, trail).await;
        emit_lifecycle(self.lifecycle_sink.as_ref(), &incident).await;

        enqueue_notify_event(
            self.notify_engine.as_ref(),
            &incident,
            ALERT_TRIGGERED_EVENT,
        )
        .await;

        self.assign_to_group(&incident, now).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::no_data_text;
    use crate::shared::i18n_language_seed::seeded_pack;

    #[test]
    fn a_check_without_results_says_where_and_why_in_the_workspace_language() {
        let places = ["Frankfurt".to_string(), "Singapore".to_string()];

        let chinese = seeded_pack("synthetics.", "zh-cn");
        assert_eq!(
            no_data_text(
                &chinese,
                "Checkout API",
                &places,
                3,
                Some("no_agent_online")
            )
            .as_deref(),
            Some(
                "检查 Checkout API 没有结果：Frankfurt、Singapore 连续 3 次没有运行，那里没有在线的 Agent。"
            )
        );

        let english = seeded_pack("synthetics.", "en-us");
        assert_eq!(
            no_data_text(
                &english,
                "Checkout API",
                &places[..1],
                1,
                Some("agents_busy")
            )
            .as_deref(),
            Some(
                "Synthetic check Checkout API has no results from Frankfurt (1 missed in a row): \
                 every Agent there was busy."
            )
        );

        // A reason it has no words for is left out, and so is the sentence without the table.
        assert_eq!(
            no_data_text(
                &english,
                "Checkout API",
                &places[..1],
                2,
                Some("new_reason")
            )
            .as_deref(),
            Some("Synthetic check Checkout API has no results from Frankfurt (2 missed in a row).")
        );
        assert!(no_data_text(&Default::default(), "Checkout API", &places, 2, None).is_none());

        // Every reason a skipped run gives has its words.
        for reason in [
            "no_agent_online",
            "no_capable_agent",
            "agent_too_old",
            "agents_busy",
        ] {
            for pack in [&chinese, &english] {
                assert!(
                    pack.get(&format!("synthetics.skip_reason.{reason}"))
                        .is_some()
                );
            }
        }
    }
}
