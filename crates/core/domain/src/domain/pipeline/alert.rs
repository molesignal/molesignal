// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Alert event pipelines (`function_steps.mode = "alert"`): steps that change an alert event, the
//! way a realtime pipeline changes a log as it arrives. Saving checks the configuration and every
//! run reads it through [`AlertConfig::parse`].
//!
//! - **Stage** (`alert.stage`): `open` runs before the incident opens and changes the incident;
//!   `notify` runs when a notification policy is about to send, on a copy of the event, and changes
//!   only what that policy sends.
//! - **Conditions** (`alert`): the sources (`rule`, `external`, `synthetic`, `no_data`, and for the
//!   `notify` stage `manual`), severities and labels (each equal) an event must have, and the times
//!   it opens at (`time`: inside or outside some weekly hours, in a timezone). A `notify` pipeline
//!   also names the policies it is for (`policies`). Nothing given means every event.
//! - **Steps**: a VRL step changes the event (`summary`, `severity`, `priority`, `urgency`,
//!   `labels`, `annotations`, `affected_services`) or drops it with `abort`, and reads where it came
//!   from as `%rule_id`, `%rule_name` and `%source`. Three kinds go outside the process and so run
//!   only when an incident opens: an AI summary gives the event to the organization's model with its
//!   prompt, an HTTP enrich step posts it to an HTTP connector and takes what the answer changes, and
//!   a query enrich step ([`query`]) writes the rows of a query into an annotation.
//! - **Order** (`alert.order`): pipelines run lowest first; a tie goes by name.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    domain::{
        alerting::{incident::Severity, time_window::TimeCondition},
        query::QueryLanguage,
        stream::StreamType,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod fields;
pub mod query;

pub use fields::{AlertFieldChange, AlertFields};
pub use query::EnrichQuery;

pub const ALERT_MODE: &str = "alert";

/// The annotation an AI summary step writes unless it names another.
pub const DEFAULT_SUMMARY_ANNOTATION: &str = "ai_summary";

/// At most this many steps in one alert pipeline.
pub const MAX_ALERT_STEPS: usize = 20;

/// The longest prompt of an AI summary step.
const MAX_PROMPT_LENGTH: usize = 8 * 1024;

const MAX_NAME_LENGTH: usize = 255;

pub fn is_alert(config: &Value) -> bool {
    config.get("mode").and_then(Value::as_str) == Some(ALERT_MODE)
}

/// Where an alert event comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertEventSource {
    /// An alert rule fired.
    Rule,
    /// An external alert source sent it.
    External,
    /// A synthetic check failed.
    Synthetic,
    /// A rule found no data.
    NoData,
    /// Someone opened the incident by hand. Such an event is never changed before it opens: what a
    /// person wrote stays. A `notify` pipeline can still change what a policy sends about it.
    Manual,
}

impl AlertEventSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::External => "external",
            Self::Synthetic => "synthetic",
            Self::NoData => "no_data",
            Self::Manual => "manual",
        }
    }
}

/// When an alert pipeline runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertStage {
    /// Before the incident opens: what the steps leave is the incident.
    #[default]
    Open,
    /// Before a notification policy sends: what the steps leave is only that policy's notification.
    Notify,
}

/// One step of an alert pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertStep {
    Vrl {
        name: String,
        script: String,
    },
    /// Asks the organization's model about the event with `prompt`; the answer goes into the
    /// annotation `annotation`.
    Summary {
        name: String,
        prompt: String,
        annotation: String,
    },
    /// Posts the event to one of the organization's HTTP connectors and takes what the answer
    /// changes. An answer that fails leaves the event as it was before this step.
    Http {
        name: String,
        connector_id: Id,
    },
    /// Runs a query with values of the event in it and writes its rows into an annotation. A query
    /// that fails leaves the event as it was before this step.
    Query {
        name: String,
        query: EnrichQuery,
    },
}

impl AlertStep {
    pub fn name(&self) -> &str {
        match self {
            Self::Vrl { name, .. }
            | Self::Summary { name, .. }
            | Self::Http { name, .. }
            | Self::Query { name, .. } => name,
        }
    }

    /// Whether the step goes outside the process (a model, an HTTP service, a query): such steps run
    /// once, when an incident opens.
    pub fn goes_out(&self) -> bool {
        !matches!(self, Self::Vrl { .. })
    }
}

/// What an alert pipeline is for, and what it does.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AlertConfig {
    pub stage: AlertStage,
    /// The notification policies a `notify` pipeline is for. Empty: every policy of an alert event.
    pub policies: Vec<Id>,
    /// Empty: events of every source.
    pub sources: Vec<AlertEventSource>,
    /// Empty: events of every severity.
    pub severities: Vec<Severity>,
    /// Labels an event must have, each with this value.
    pub labels: BTreeMap<String, String>,
    /// When the event must open: inside or outside these weekly hours. None means any time.
    pub time: Option<TimeCondition>,
    pub order: i64,
    pub steps: Vec<AlertStep>,
}

#[derive(Deserialize)]
struct RawConditions {
    #[serde(default)]
    stage: AlertStage,
    #[serde(default)]
    policies: Vec<String>,
    #[serde(default)]
    sources: Vec<AlertEventSource>,
    #[serde(default)]
    severities: Vec<String>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    time: Option<TimeCondition>,
    #[serde(default)]
    order: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStep {
    #[serde(default, alias = "name")]
    transform_name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    script: String,
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    annotation: String,
    #[serde(default)]
    connector_id: String,
    #[serde(default)]
    language: Option<QueryLanguage>,
    #[serde(default)]
    stream: String,
    #[serde(default)]
    stream_type: Option<String>,
    #[serde(default)]
    statement: String,
    #[serde(default)]
    window_secs: Option<u32>,
    #[serde(default)]
    max_rows: Option<u32>,
}

impl AlertConfig {
    pub fn parse(config: &Value) -> Result<Self> {
        if !is_alert(config) {
            return Err(Error::invalid("not an alert pipeline"));
        }

        let conditions: RawConditions = serde_json::from_value(
            config
                .get("alert")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        )
        .map_err(|e| Error::invalid(format!("invalid alert pipeline conditions: {e}")))?;

        let severities = conditions
            .severities
            .iter()
            .map(|word| {
                Severity::from_label(word)
                    .ok_or_else(|| Error::invalid(format!("unknown severity `{word}`")))
            })
            .collect::<Result<Vec<_>>>()?;

        let labels = conditions
            .labels
            .into_iter()
            .map(|(key, value)| (key.trim().to_owned(), value))
            .filter(|(key, _)| !key.is_empty())
            .collect();

        let raw: Vec<RawStep> = serde_json::from_value(
            config
                .get("steps")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
        .map_err(|_| Error::invalid("invalid alert pipeline steps"))?;

        if raw.len() > MAX_ALERT_STEPS {
            return Err(Error::invalid(format!(
                "an alert pipeline has at most {MAX_ALERT_STEPS} steps"
            )));
        }

        let steps = raw.into_iter().map(step).collect::<Result<Vec<_>>>()?;

        if let Some(time) = &conditions.time {
            time.validate()?;
        }

        let policies: Vec<Id> = conditions
            .policies
            .iter()
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(Id::from_string)
            .collect();

        match conditions.stage {
            AlertStage::Open => {
                if !policies.is_empty() {
                    return Err(Error::invalid(
                        "only a pipeline that runs before notifications names policies",
                    ));
                }

                if conditions.sources.contains(&AlertEventSource::Manual) {
                    return Err(Error::invalid(
                        "an incident opened by hand opens as it was written: no pipeline changes it",
                    ));
                }
            }
            AlertStage::Notify => {
                if let Some(step) = steps.iter().find(|step| step.goes_out()) {
                    return Err(Error::invalid(format!(
                        "step `{}` goes outside MoleSignal, which a step does only when an incident \
                         opens, not for every notification",
                        step.name()
                    )));
                }
            }
        }

        Ok(Self {
            stage: conditions.stage,
            policies,
            sources: conditions.sources,
            severities,
            labels,
            time: conditions.time,
            order: conditions.order,
            steps,
        })
    }

    /// Whether an event of this source, severity and labels, opening at `at`, is one this
    /// pipeline is for.
    pub fn applies(
        &self,
        source: AlertEventSource,
        severity: Severity,
        labels: &BTreeMap<String, String>,
        at: TimestampMicros,
    ) -> bool {
        (self.sources.is_empty() || self.sources.contains(&source))
            && (self.severities.is_empty() || self.severities.contains(&severity))
            && self
                .labels
                .iter()
                .all(|(key, value)| labels.get(key) == Some(value))
            && self.time.as_ref().is_none_or(|time| time.holds(at))
    }

    /// Whether this pipeline is for what `policy` sends.
    pub fn for_policy(&self, policy: &Id) -> bool {
        self.stage == AlertStage::Notify
            && (self.policies.is_empty() || self.policies.contains(policy))
    }
}

fn step(raw: RawStep) -> Result<AlertStep> {
    let name = raw.transform_name.trim().to_owned();

    match raw.kind.as_str() {
        "" | "vrl" => {
            if raw.script.trim().is_empty() {
                return Err(Error::invalid("VRL script is required"));
            }

            Ok(AlertStep::Vrl {
                name,
                script: raw.script,
            })
        }
        "ai_summary" => {
            let prompt = raw.prompt.trim();

            if prompt.is_empty() {
                return Err(Error::invalid("an AI summary step needs a prompt"));
            }

            if prompt.len() > MAX_PROMPT_LENGTH {
                return Err(Error::invalid(format!(
                    "an AI summary prompt is at most {MAX_PROMPT_LENGTH} bytes"
                )));
            }

            let annotation = match raw.annotation.trim() {
                "" => DEFAULT_SUMMARY_ANNOTATION.to_owned(),
                name if name.len() > MAX_NAME_LENGTH => {
                    return Err(Error::invalid(format!(
                        "annotation names are at most {MAX_NAME_LENGTH} bytes"
                    )));
                }
                name => name.to_owned(),
            };

            Ok(AlertStep::Summary {
                name,
                prompt: prompt.to_owned(),
                annotation,
            })
        }
        "http_enrich" => {
            let connector_id = raw.connector_id.trim();

            if connector_id.is_empty() {
                return Err(Error::invalid(
                    "an HTTP enrich step needs an HTTP connector",
                ));
            }

            Ok(AlertStep::Http {
                name,
                connector_id: Id::from_string(connector_id),
            })
        }
        "query_enrich" => {
            let stream_type = match raw.stream_type.as_deref().unwrap_or("logs") {
                "logs" => StreamType::LOGS,
                "metrics" => StreamType::METRICS,
                "traces" => StreamType::TRACES,
                other => {
                    return Err(Error::invalid(format!(
                        "a query step cannot read a stream of `{other}`"
                    )));
                }
            };

            let annotation = match raw.annotation.trim() {
                "" => query::DEFAULT_QUERY_ANNOTATION.to_owned(),
                name if name.len() > MAX_NAME_LENGTH => {
                    return Err(Error::invalid(format!(
                        "annotation names are at most {MAX_NAME_LENGTH} bytes"
                    )));
                }
                name => name.to_owned(),
            };

            let query = EnrichQuery {
                language: raw.language.unwrap_or(QueryLanguage::Sql),
                stream: raw.stream.trim().to_owned(),
                stream_type,
                statement: raw.statement.trim().to_owned(),
                window_secs: raw.window_secs.unwrap_or(query::DEFAULT_QUERY_WINDOW_SECS),
                max_rows: raw.max_rows.unwrap_or(query::DEFAULT_QUERY_ROWS),
                annotation,
            };

            Ok(AlertStep::Query {
                name,
                query: query.checked()?,
            })
        }
        "builtin" => Err(Error::invalid(
            "a routing step does not apply to alert events",
        )),
        other => Err(Error::invalid(format!("unknown alert step kind `{other}`"))),
    }
}

/// An enabled alert pipeline of an organization.
#[derive(Debug, Clone)]
pub struct AlertPipeline {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub config: AlertConfig,
    pub updated_at: TimestampMicros,
}

/// What became of one alert event in one pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertOutcomeKind {
    /// The steps ran; they may or may not have changed the event.
    Processed,
    /// A step dropped the event: no incident opened, or the policy did not send.
    Dropped,
    /// A step failed: the event went on as it came into the pipeline, or for a step that goes
    /// outside MoleSignal as it came to that step.
    Failed,
}

/// One event's way through one pipeline, as the pipeline keeps count of it.
#[derive(Debug, Clone)]
pub struct AlertOutcome {
    pub kind: AlertOutcomeKind,
    pub at: TimestampMicros,
    /// The rule or source the event came from, to say which one in the pipeline's record.
    pub rule_name: String,
    /// Why the event was dropped or failed.
    pub reason: Option<String>,
}

#[async_trait]
pub trait AlertPipelineRepository: Send + Sync {
    /// The organization's enabled alert pipelines, in the order they run. Pipelines whose saved
    /// configuration cannot be read are left out.
    async fn enabled(&self, org: &Id) -> Result<Vec<AlertPipeline>>;

    /// Counts what became of an event in a pipeline, and keeps the latest drop and failure.
    async fn record(&self, pipeline: &Id, outcome: AlertOutcome) -> Result<()>;
}

/// Posts an alert event to an HTTP connector, for an HTTP enrich step.
#[async_trait]
pub trait AlertEnricher: Send + Sync {
    /// The JSON object the organization's connector answers `body` with; `None` when it answers with
    /// nothing. Anything else, a refusal or no answer in time is an error.
    async fn call(
        &self,
        org: &Id,
        connector: &Id,
        body: &Value,
    ) -> Result<Option<Map<String, Value>>>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn conditions_and_steps_are_read_with_their_defaults() {
        let config = AlertConfig::parse(&json!({
            "mode": "alert",
            "alert": {
                "sources": ["rule", "external"],
                "severities": ["critical", "warn"],
                "labels": { " env ": "prod", "": "x" },
                "order": 5,
            },
            "steps": [
                { "transformName": "env", "script": ".labels.team = \"payments\"" },
                { "transformName": "summary", "kind": "ai_summary", "prompt": " Explain it " },
            ],
        }))
        .unwrap();

        assert_eq!(
            config.sources,
            vec![AlertEventSource::Rule, AlertEventSource::External]
        );
        assert_eq!(
            config.severities,
            vec![Severity::Critical, Severity::Warning]
        );
        assert_eq!(config.labels.get("env").map(String::as_str), Some("prod"));
        assert_eq!(config.labels.len(), 1);
        assert_eq!(config.order, 5);
        assert_eq!(
            config.steps[1],
            AlertStep::Summary {
                name: "summary".into(),
                prompt: "Explain it".into(),
                annotation: DEFAULT_SUMMARY_ANNOTATION.into(),
            }
        );
        assert!(config.steps[1].goes_out() && !config.steps[0].goes_out());

        let bare = AlertConfig::parse(&json!({ "mode": "alert" })).unwrap();

        assert!(bare.steps.is_empty() && bare.sources.is_empty() && bare.order == 0);
        assert_eq!(bare.stage, AlertStage::Open);
    }

    #[test]
    fn a_step_can_call_a_connector_or_write_the_rows_of_a_query_into_an_annotation() {
        let config = AlertConfig::parse(&json!({
            "mode": "alert",
            "steps": [
                { "transformName": "cmdb", "kind": "http_enrich", "connectorId": " c-1 " },
                {
                    "transformName": "errors",
                    "kind": "query_enrich",
                    "stream": " app_logs ",
                    "statement": " SELECT count(*) FROM app_logs WHERE host = '{{labels.host}}' ",
                },
                {
                    "kind": "query_enrich",
                    "language": "promql",
                    "stream": "node",
                    "streamType": "metrics",
                    "statement": "up{instance=\"{{labels.host}}\"}",
                    "windowSecs": 60,
                    "maxRows": 1,
                    "annotation": "up",
                },
            ],
        }))
        .unwrap();

        assert_eq!(
            config.steps[0],
            AlertStep::Http {
                name: "cmdb".into(),
                connector_id: Id::from_string("c-1"),
            }
        );

        let AlertStep::Query { query, .. } = &config.steps[1] else {
            panic!("a query step");
        };

        assert_eq!(query.language, QueryLanguage::Sql);
        assert_eq!(query.stream, "app_logs");
        assert_eq!(query.stream_type, StreamType::LOGS);
        assert_eq!(query.window_secs, query::DEFAULT_QUERY_WINDOW_SECS);
        assert_eq!(query.max_rows, query::DEFAULT_QUERY_ROWS);
        assert_eq!(query.annotation, query::DEFAULT_QUERY_ANNOTATION);

        let AlertStep::Query { query, .. } = &config.steps[2] else {
            panic!("a query step");
        };

        assert_eq!(query.language, QueryLanguage::Promql);
        assert_eq!(query.stream_type, StreamType::METRICS);
        assert_eq!((query.window_secs, query.max_rows), (60, 1));
        assert_eq!(query.annotation, "up");

        let query_step = |extra: Value| {
            let mut step = json!({
                "kind": "query_enrich",
                "stream": "app_logs",
                "statement": "SELECT 1",
            });
            step.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());

            AlertConfig::parse(&json!({ "mode": "alert", "steps": [step] }))
        };

        assert!(query_step(json!({})).is_ok());

        for extra in [
            json!({ "stream": "" }),
            json!({ "stream": "_audit" }),
            json!({ "stream": "_molesignal" }),
            json!({ "streamType": "profiles" }),
            json!({ "language": "promql" }),
            json!({ "statement": " " }),
            json!({ "windowSecs": 0 }),
            json!({ "windowSecs": query::MAX_QUERY_WINDOW_SECS + 1 }),
            json!({ "maxRows": 0 }),
            json!({ "maxRows": query::MAX_QUERY_ROWS + 1 }),
        ] {
            assert!(query_step(extra.clone()).is_err(), "{extra}");
        }

        assert!(
            AlertConfig::parse(&json!({ "mode": "alert", "steps": [{ "kind": "http_enrich" }] }))
                .is_err()
        );
    }

    #[test]
    fn a_pipeline_before_notifications_names_its_policies_and_stays_in_the_process() {
        let notify = |alert: Value, steps: Value| {
            AlertConfig::parse(&json!({ "mode": "alert", "alert": alert, "steps": steps }))
        };

        let config = notify(
            json!({ "stage": "notify", "policies": [" p-1 ", ""], "sources": ["manual"] }),
            json!([{ "script": ".summary = \"x\"" }]),
        )
        .unwrap();

        assert_eq!(config.stage, AlertStage::Notify);
        assert_eq!(config.policies, vec![Id::from_string("p-1")]);
        assert!(config.for_policy(&Id::from_string("p-1")));
        assert!(!config.for_policy(&Id::from_string("p-2")));

        let every = notify(json!({ "stage": "notify" }), json!([])).unwrap();
        assert!(every.for_policy(&Id::from_string("p-2")));

        // A pipeline that runs when incidents open is for no policy.
        let opening = notify(json!({}), json!([])).unwrap();
        assert!(!opening.for_policy(&Id::from_string("p-2")));

        for step in [
            json!({ "kind": "ai_summary", "prompt": "Explain" }),
            json!({ "kind": "http_enrich", "connectorId": "c-1" }),
            json!({ "kind": "query_enrich", "stream": "s", "statement": "SELECT 1" }),
        ] {
            assert!(notify(json!({ "stage": "notify" }), json!([step])).is_err());
        }

        assert!(notify(json!({ "policies": ["p-1"] }), json!([])).is_err());
        assert!(notify(json!({ "sources": ["manual"] }), json!([])).is_err());
    }

    #[test]
    fn what_a_pipeline_cannot_do_is_refused_when_saved() {
        for config in [
            json!({ "mode": "realtime" }),
            json!({ "mode": "alert", "alert": { "severities": ["fatal"] } }),
            json!({ "mode": "alert", "alert": { "sources": ["email"] } }),
            json!({ "mode": "alert", "steps": [{ "script": " " }] }),
            json!({ "mode": "alert", "steps": [{ "kind": "ai_summary" }] }),
            json!({ "mode": "alert", "steps": [{ "kind": "builtin", "operation": "route" }] }),
            json!({ "mode": "alert", "steps": [{ "kind": "lua", "script": "x" }] }),
        ] {
            assert!(AlertConfig::parse(&config).is_err(), "{config}");
        }

        let many: Vec<Value> = (0..=MAX_ALERT_STEPS)
            .map(|_| json!({ "script": "." }))
            .collect();

        assert!(AlertConfig::parse(&json!({ "mode": "alert", "steps": many })).is_err());
    }

    #[test]
    fn a_pipeline_is_for_the_events_its_conditions_name() {
        let config = AlertConfig::parse(&json!({
            "mode": "alert",
            "alert": { "sources": ["rule"], "severities": ["critical"], "labels": { "env": "prod" } },
        }))
        .unwrap();
        let prod: BTreeMap<String, String> = [("env".to_string(), "prod".to_string())].into();
        let dev: BTreeMap<String, String> = [("env".to_string(), "dev".to_string())].into();

        let now = TimestampMicros(0);

        assert!(config.applies(AlertEventSource::Rule, Severity::Critical, &prod, now));
        assert!(!config.applies(AlertEventSource::External, Severity::Critical, &prod, now));
        assert!(!config.applies(AlertEventSource::Rule, Severity::Error, &prod, now));
        assert!(!config.applies(AlertEventSource::Rule, Severity::Critical, &dev, now));
        assert!(AlertConfig::default().applies(
            AlertEventSource::NoData,
            Severity::Info,
            &dev,
            now
        ));

        // Only at night (outside 09:00–18:00 UTC): 1970-01-01 01:00 is night, 10:00 is not.
        let nightly = AlertConfig::parse(&json!({
            "mode": "alert",
            "alert": { "time": {
                "mode": "outside",
                "timezone": "UTC",
                "ranges": [{ "weekdayMask": 127, "start": "09:00", "end": "18:00" }],
            } },
        }))
        .unwrap();
        let hour = |hour: i64| TimestampMicros(hour * 3_600_000_000);

        assert!(nightly.applies(AlertEventSource::Rule, Severity::Info, &dev, hour(1)));
        assert!(!nightly.applies(AlertEventSource::Rule, Severity::Info, &dev, hour(10)));
        assert!(
            AlertConfig::parse(&json!({
                "mode": "alert",
                "alert": { "time": { "mode": "inside", "timezone": "Nowhere/Land", "ranges": [] } },
            }))
            .is_err()
        );
    }
}
