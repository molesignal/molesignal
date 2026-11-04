// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警流水线：组织启用着的告警流水线按顺序处理告警事件（配置见 `domain::pipeline::alert`）。
//! 开事件之前跑的在这里，定下事件本身；通知策略发送之前跑的见 [`notify`]，只改这条策略发出的那一份。
//!
//! - 每条流水线先看条件（来源、严重度、标签、开出来的时段），适用的才把事件交给步骤。VRL 步骤改
//!   `summary`、`severity`、`priority`、`urgency`、`labels`、`annotations`、`affected_services`，
//!   或用 `abort` 丢弃；读 `%rule_id`、`%rule_name`、`%source`、`%event`。AI 摘要步骤把事件交给
//!   组织的模型，回答写进注解；调用接口和查询补充见 [`enrich`]。
//! - 一条流水线出错（步骤失败、改出来的事件不合法），它的改动作废，事件照进来时的样子交给下一条。
//!   调用接口、查询补充失败只算这一步没做：事件照这一步之前的样子往下走。
//! - 丢弃就不开事件。
//! - 每条流水线记下处理、丢弃、出错的次数，和最近一次丢弃与出错；改了什么、出了什么错随事件交回去
//!   （[`Processed::Kept`]），事件写库后记进它的时间线。
//! - 来源重新报出严重度时（[`AlertPipelines::severity`]），这一轮进来的事件和开事件时一样构造，
//!   再跑一遍不出进程的步骤，只取它们定的严重度：步骤看到的和开事件时是同样的事件，标签和注解在
//!   开事件时已经定下，不再改。这时的丢弃不算数，事件已经开着。

use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::{
    domain::{
        alerting::incident::{Incident, Severity},
        function::{Function, FunctionExecutor, FunctionLanguage, FunctionOutcome},
        pipeline::alert::{
            AlertEnricher, AlertEventSource, AlertFieldChange, AlertFields, AlertOutcome,
            AlertOutcomeKind, AlertPipeline, AlertPipelineRepository, AlertStage, AlertStep,
            fields::MAX_RECORDED_CHANGES,
        },
        query::QueryEngine,
    },
    shared::{ids::Id, time::TimestampMicros},
};

mod enrich;
pub mod notify;

/// Where an AI summary step's answer is put for the moment, before it goes into the annotation.
const SUMMARY_FIELD: &str = "_ai_summary";

/// What the pipelines made of an event.
#[derive(Debug)]
pub enum Processed {
    /// The incident to open, and what each pipeline did to it, for its timeline.
    Kept(Box<Incident>, Vec<PipelineTrace>),
    /// A step dropped the event: no incident opens. Says which pipeline and step.
    Dropped(String),
}

/// What one pipeline did to an event, as the incident's timeline records it. A pipeline that changed
/// nothing and met no trouble leaves none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineTrace {
    pipeline_id: Id,
    pipeline: String,
    changes: Vec<AlertFieldChange>,
    error: Option<String>,
}

impl PipelineTrace {
    fn of(
        pipeline: &AlertPipeline,
        changes: Vec<AlertFieldChange>,
        error: Option<String>,
    ) -> Option<Self> {
        (!changes.is_empty() || error.is_some()).then(|| Self {
            pipeline_id: pipeline.id.clone(),
            pipeline: pipeline.name.clone(),
            changes,
            error,
        })
    }

    /// The timeline entry's `detail`: the changes up to a limit, and how many more there were.
    pub fn detail(&self) -> Value {
        let more = self.changes.len().saturating_sub(MAX_RECORDED_CHANGES);
        let shown = &self.changes[..self.changes.len().min(MAX_RECORDED_CHANGES)];

        json!({
            "pipelineId": self.pipeline_id,
            "pipeline": self.pipeline,
            "changes": shown,
            "more": more,
            "error": self.error,
        })
    }
}

/// What an event is and where it came from, for the steps to read.
#[derive(Debug, Clone, Copy)]
pub struct AlertOrigin<'a> {
    pub source: AlertEventSource,
    pub rule_name: &'a str,
}

/// A pipeline's way through its steps: the event as they left it, or why it stopped.
enum Passage {
    /// `trouble`: a step that goes outside the process failed, and the others went on without it.
    Done {
        event: Value,
        trouble: Option<String>,
    },
    Dropped(String),
    Failed(String),
}

pub struct AlertPipelines {
    repository: Arc<dyn AlertPipelineRepository>,
    executor: Arc<dyn FunctionExecutor>,
    /// Where an HTTP enrich step posts; without one such a step fails.
    enricher: Option<Arc<dyn AlertEnricher>>,
    /// Where a query enrich step reads; without one such a step fails.
    queries: Option<Arc<dyn QueryEngine>>,
    /// How a drop before a notification is told; without it, by the names alone.
    drop_texts: Option<notify::DropTexts>,
}

impl AlertPipelines {
    pub fn new(
        repository: Arc<dyn AlertPipelineRepository>,
        executor: Arc<dyn FunctionExecutor>,
    ) -> Self {
        Self {
            repository,
            executor,
            enricher: None,
            queries: None,
            drop_texts: None,
        }
    }

    /// HTTP enrich steps post through `enricher`, query enrich steps read through `queries`.
    pub fn with_enrichment(
        mut self,
        enricher: Arc<dyn AlertEnricher>,
        queries: Arc<dyn QueryEngine>,
    ) -> Self {
        self.enricher = Some(enricher);
        self.queries = Some(queries);
        self
    }

    /// Runs the organization's alert pipelines on an event about to open an incident.
    pub async fn process(&self, incident: Incident, origin: AlertOrigin<'_>) -> Processed {
        let pipelines = match self.repository.enabled(&incident.org_id).await {
            Ok(pipelines) => pipelines,
            Err(error) => {
                tracing::warn!(org_id = %incident.org_id, %error, "alert pipelines unreadable; the event opens as it came");
                return Processed::Kept(Box::new(incident), Vec::new());
            }
        };

        let mut incident = incident;
        let mut trail = Vec::new();

        for pipeline in pipelines
            .iter()
            .filter(|pipeline| pipeline.config.stage == AlertStage::Open)
        {
            if !pipeline.config.applies(
                origin.source,
                incident.severity,
                &incident.labels,
                incident.created_at,
            ) {
                continue;
            }

            let fields = AlertFields::of(&incident);
            let metadata = metadata(&incident.rule_id, origin, "opened");

            let passage = self
                .pass(pipeline, &fields, &metadata, incident.created_at, true)
                .await;

            let (kind, reason) = match passage {
                Passage::Done { event, trouble } => match fields.changed(&event) {
                    Ok(next) => {
                        trail.extend(PipelineTrace::of(
                            pipeline,
                            fields.changes_to(&next),
                            trouble.clone(),
                        ));

                        next.apply_to(&mut incident);

                        match trouble {
                            Some(trouble) => (AlertOutcomeKind::Failed, Some(trouble)),
                            None => (AlertOutcomeKind::Processed, None),
                        }
                    }
                    Err(reason) => {
                        trail.extend(PipelineTrace::of(
                            pipeline,
                            Vec::new(),
                            Some(reason.clone()),
                        ));

                        (AlertOutcomeKind::Failed, Some(reason))
                    }
                },
                Passage::Dropped(step) => {
                    let reason = format!("dropped by step `{step}`");
                    self.record(
                        pipeline,
                        AlertOutcomeKind::Dropped,
                        origin.rule_name,
                        Some(reason),
                    );

                    return Processed::Dropped(format!(
                        "dropped by alert pipeline `{}`, step `{step}`",
                        pipeline.name
                    ));
                }
                Passage::Failed(reason) => {
                    trail.extend(PipelineTrace::of(
                        pipeline,
                        Vec::new(),
                        Some(reason.clone()),
                    ));

                    (AlertOutcomeKind::Failed, Some(reason))
                }
            };

            if let Some(reason) = &reason {
                tracing::warn!(pipeline_id = %pipeline.id, %reason, "alert pipeline failed");
            }

            self.record(pipeline, kind, origin.rule_name, reason);
        }

        Processed::Kept(Box::new(incident), trail)
    }

    /// The severity the pipelines give an open incident's event as it comes in again: `incoming` is
    /// built as it was when the incident opened, with the severity its source finds now.
    pub async fn severity(&self, incoming: &Incident, origin: AlertOrigin<'_>) -> Severity {
        let Ok(pipelines) = self.repository.enabled(&incoming.org_id).await else {
            return incoming.severity;
        };

        let mut current = incoming.clone();

        for pipeline in pipelines
            .iter()
            .filter(|pipeline| pipeline.config.stage == AlertStage::Open)
        {
            if !pipeline.config.applies(
                origin.source,
                current.severity,
                &current.labels,
                current.created_at,
            ) {
                continue;
            }

            let fields = AlertFields::of(&current);
            let metadata = metadata(&current.rule_id, origin, "severity_changed");

            if let Passage::Done { event, .. } = self
                .pass(pipeline, &fields, &metadata, current.created_at, false)
                .await
                && let Ok(next) = fields.changed(&event)
            {
                next.apply_to(&mut current);
            }
        }

        current.severity
    }

    /// Runs one pipeline's steps on the event. Without `outward`, the steps that go outside the
    /// process are left out. `at` is the moment the event is about, which a query looks back from.
    async fn pass(
        &self,
        pipeline: &AlertPipeline,
        fields: &AlertFields,
        metadata: &Map<String, Value>,
        at: TimestampMicros,
        outward: bool,
    ) -> Passage {
        let mut event = fields.event();
        let mut troubles = Vec::new();

        for (index, step) in pipeline.config.steps.iter().enumerate() {
            if step.goes_out() && !outward {
                continue;
            }

            let label = if step.name().is_empty() {
                (index + 1).to_string()
            } else {
                step.name().to_owned()
            };

            let function = match step {
                AlertStep::Vrl { name, script } => function(
                    pipeline,
                    index,
                    name,
                    FunctionLanguage::Vrl,
                    script,
                    Value::Null,
                ),
                AlertStep::Summary { name, prompt, .. } => function(
                    pipeline,
                    index,
                    name,
                    FunctionLanguage::Llm,
                    prompt,
                    json!({ "outputField": SUMMARY_FIELD }),
                ),
                AlertStep::Http { connector_id, .. } => {
                    if let Err(reason) = self
                        .call_connector(pipeline, connector_id, fields, &mut event, metadata)
                        .await
                    {
                        troubles.push(format!("step `{label}`: {reason}"));
                    }

                    continue;
                }
                AlertStep::Query { query, .. } => {
                    if let Err(reason) = self
                        .run_query(pipeline, query, &mut event, metadata, at)
                        .await
                    {
                        troubles.push(format!("step `{label}`: {reason}"));
                    }

                    continue;
                }
            };

            match self
                .executor
                .run_with_metadata(&function, &mut event, metadata)
                .await
            {
                Ok(FunctionOutcome::Kept) => {}
                Ok(FunctionOutcome::Dropped) => return Passage::Dropped(label),
                Err(error) => return Passage::Failed(format!("step `{label}`: {error}")),
            }

            if let AlertStep::Summary { annotation, .. } = step {
                let answer = event
                    .as_object_mut()
                    .and_then(|fields| fields.remove(SUMMARY_FIELD));

                if let (Some(Value::String(answer)), Some(annotations)) = (
                    answer,
                    event.get_mut("annotations").and_then(Value::as_object_mut),
                ) {
                    annotations.insert(annotation.clone(), Value::String(answer));
                }
            }
        }

        Passage::Done {
            event,
            trouble: (!troubles.is_empty()).then(|| troubles.join("; ")),
        }
    }

    /// Counts what became of an event in a pipeline, without holding the incident back.
    fn record(
        &self,
        pipeline: &AlertPipeline,
        kind: AlertOutcomeKind,
        rule_name: &str,
        reason: Option<String>,
    ) {
        let repository = self.repository.clone();
        let id = pipeline.id.clone();

        let outcome = AlertOutcome {
            kind,
            at: TimestampMicros::now(),
            rule_name: rule_name.to_owned(),
            reason,
        };

        tokio::spawn(async move {
            if let Err(error) = repository.record(&id, outcome).await {
                tracing::debug!(pipeline_id = %id, %error, "alert pipeline outcome not recorded");
            }
        });
    }
}

/// Where the event came from, as the steps read it.
fn metadata(rule_id: &Id, origin: AlertOrigin<'_>, event_name: &str) -> Map<String, Value> {
    let mut metadata = Map::new();
    metadata.insert("rule_id".into(), rule_id.to_string().into());
    metadata.insert("rule_name".into(), origin.rule_name.into());
    metadata.insert("source".into(), origin.source.as_str().into());
    metadata.insert("event".into(), event_name.into());
    metadata
}

fn function(
    pipeline: &AlertPipeline,
    index: usize,
    name: &str,
    language: FunctionLanguage,
    source: &str,
    params_schema: Value,
) -> Function {
    Function {
        id: Id(format!("{}:{index}", pipeline.id)),
        org_id: pipeline.org_id.clone(),
        name: name.to_owned(),
        language,
        source: source.to_owned(),
        params_schema,
        created_at: pipeline.updated_at,
        updated_at: pipeline.updated_at,
    }
}

/// The event the steps see.
pub fn event_of(incident: &Incident) -> Value {
    AlertFields::of(incident).event()
}

/// The incident as the steps left its event, or why that event cannot be one.
pub fn changed(incident: &Incident, event: &Value) -> std::result::Result<Incident, String> {
    let fields = AlertFields::of(incident).changed(event)?;
    let mut changed = incident.clone();
    fields.apply_to(&mut changed);
    Ok(changed)
}

#[cfg(test)]
mod tests;
