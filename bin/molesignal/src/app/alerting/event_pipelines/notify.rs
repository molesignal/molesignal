// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 通知前处理：通知策略发送一条告警通知之前，适用的告警流水线（`alert.stage = notify`）处理事件的
//! 副本，改动只影响这条策略发出的通知；丢弃就是这条策略不发。条件和开事件时一样，另外可以点名策略。
//! 步骤都在进程内：走出进程的步骤只在开事件时跑（`AlertConfig::parse` 保存时就拒绝）。
//! 一条流水线出错，它的改动作废，副本照进来时的样子交给下一条。

use std::sync::Arc;

use serde_json::{Map, Value};

use super::{AlertPipelines, Passage};
use crate::{
    domain::{
        alerting::incident::{
            CONDITION_LABEL, EXTERNAL_SOURCE, MANUAL_SOURCE, NO_DATA_CONDITION, SOURCE_LABEL,
            SYNTHETIC_SOURCE,
        },
        i18n_language::{FALLBACK_LANGUAGE_CODE, I18nLanguageRepository, LanguagePack},
        pipeline::alert::{
            AlertEventSource, AlertFields, AlertOutcomeKind, AlertPipeline, AlertStage,
        },
    },
    infra::persistence::repositories::workspace_preference_defaults::WorkspacePreferenceDefaultsRepository,
    shared::{ids::Id, time::TimestampMicros},
};

const LANGUAGE_KEY_PREFIX: &str = "alerting.pipeline.";
const DROPPED_LANGUAGE_KEY: &str = "alerting.pipeline.dropped";

/// Where the words for a drop come from: the delivery record says why in the workspace's language.
pub(super) struct DropTexts {
    i18n: Arc<dyn I18nLanguageRepository>,
    workspace_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository>,
}

/// What a notification is about, for the conditions and the steps to read.
#[derive(Debug, Clone, Copy)]
pub struct NotifyFacts<'a> {
    pub policy_id: &'a Id,
    pub policy_name: &'a str,
    pub rule_id: &'a str,
    pub rule_name: &'a str,
    /// `triggered`, `escalated`, `acknowledged` or `resolved`.
    pub event_name: &'a str,
    pub at: TimestampMicros,
}

/// What a policy's pipelines made of a notification's copy of the event.
#[derive(Debug, PartialEq, Eq)]
pub enum BeforeNotify {
    Kept(Box<AlertFields>),
    /// A step dropped it: this policy does not send. Says which pipeline and step.
    Dropped(String),
}

impl AlertPipelines {
    /// A drop before a notification is told in the workspace's language.
    pub fn with_language(
        mut self,
        i18n: Arc<dyn I18nLanguageRepository>,
        workspace_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository>,
    ) -> Self {
        self.drop_texts = Some(DropTexts {
            i18n,
            workspace_defaults,
        });
        self
    }

    /// The organization's enabled pipelines for what `policy` sends, in the order they run.
    pub async fn for_policy(&self, org: &Id, policy: &Id) -> Vec<AlertPipeline> {
        match self.repository.enabled(org).await {
            Ok(pipelines) => pipelines
                .into_iter()
                .filter(|pipeline| {
                    pipeline.config.stage == AlertStage::Notify
                        && pipeline.config.for_policy(policy)
                })
                .collect(),
            Err(error) => {
                tracing::warn!(org_id = %org, %error, "alert pipelines unreadable; the notification goes as it is");
                Vec::new()
            }
        }
    }

    /// Runs `pipelines` on a copy of the event a notification is about.
    pub async fn before_notify(
        &self,
        pipelines: &[AlertPipeline],
        fields: AlertFields,
        facts: NotifyFacts<'_>,
    ) -> BeforeNotify {
        let source = source_of(&fields);
        let metadata = metadata(facts, source);
        let mut fields = fields;

        for pipeline in pipelines {
            if !pipeline
                .config
                .applies(source, fields.severity, &fields.labels, facts.at)
            {
                continue;
            }

            let passage = self
                .pass(pipeline, &fields, &metadata, facts.at, false)
                .await;

            let (kind, reason) = match passage {
                Passage::Done { event, .. } => match fields.changed(&event) {
                    Ok(next) => {
                        fields = next;
                        (AlertOutcomeKind::Processed, None)
                    }
                    Err(reason) => (AlertOutcomeKind::Failed, Some(reason)),
                },
                Passage::Dropped(step) => {
                    let reason = format!("dropped by step `{step}`");

                    self.record(
                        pipeline,
                        AlertOutcomeKind::Dropped,
                        facts.rule_name,
                        Some(reason),
                    );

                    return BeforeNotify::Dropped(self.drop_reason(pipeline, &step).await);
                }
                Passage::Failed(reason) => (AlertOutcomeKind::Failed, Some(reason)),
            };

            if let Some(reason) = &reason {
                tracing::warn!(pipeline_id = %pipeline.id, %reason, "alert pipeline failed; its changes are left out");
            }

            self.record(pipeline, kind, facts.rule_name, reason);
        }

        BeforeNotify::Kept(Box::new(fields))
    }

    /// Why a policy did not send, for its delivery record.
    async fn drop_reason(&self, pipeline: &AlertPipeline, step: &str) -> String {
        let pack = match &self.drop_texts {
            Some(texts) => {
                let language = texts
                    .workspace_defaults
                    .get(&pipeline.org_id)
                    .await
                    .map(|defaults| defaults.language)
                    .unwrap_or_else(|_| FALLBACK_LANGUAGE_CODE.to_string());

                texts
                    .i18n
                    .load_pack(LANGUAGE_KEY_PREFIX, &language)
                    .await
                    .unwrap_or_default()
            }
            None => LanguagePack::default(),
        };

        drop_reason_text(&pack, &pipeline.name, step)
    }
}

/// Which pipeline and step dropped a notification. A pack without the words leaves the names.
pub(super) fn drop_reason_text(pack: &LanguagePack, pipeline: &str, step: &str) -> String {
    pack.render(
        DROPPED_LANGUAGE_KEY,
        &[("pipeline", pipeline), ("step", step)],
    )
    .unwrap_or_else(|_| format!("{pipeline} / {step}"))
}

/// Where an open event came from, as its labels say.
fn source_of(fields: &AlertFields) -> AlertEventSource {
    match fields.labels.get(SOURCE_LABEL).map(String::as_str) {
        Some(SYNTHETIC_SOURCE) => AlertEventSource::Synthetic,
        Some(EXTERNAL_SOURCE) => AlertEventSource::External,
        Some(MANUAL_SOURCE) => AlertEventSource::Manual,
        _ if fields.labels.get(CONDITION_LABEL).map(String::as_str) == Some(NO_DATA_CONDITION) => {
            AlertEventSource::NoData
        }
        _ => AlertEventSource::Rule,
    }
}

fn metadata(facts: NotifyFacts<'_>, source: AlertEventSource) -> Map<String, Value> {
    let mut metadata = Map::new();
    metadata.insert("rule_id".into(), facts.rule_id.into());
    metadata.insert("rule_name".into(), facts.rule_name.into());
    metadata.insert("source".into(), source.as_str().into());
    metadata.insert("event".into(), facts.event_name.into());
    metadata.insert("policy_id".into(), facts.policy_id.to_string().into());
    metadata.insert("policy_name".into(), facts.policy_name.into());
    metadata
}
