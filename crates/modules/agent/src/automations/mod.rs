// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Automations let Mole Agent perform a task without a person present.
//!
//! A rule = name + scope (organization / team / personal) + task prompt + trigger + outputs. Each
//! trigger creates a run: one answer turn is queued in a hidden session (Run, source `automation`,
//! or `incident` for incident triggers), executed as the person who last saves the rule. On
//! completion, it sends notifications and writes the incident RCA according to the outputs, and
//! records the results in the run record.
//!
//! - Triggers: Cron (with a time zone, at most once every 15 minutes), API (the rule's own token),
//!   new incidents (filtered by severity, alert rule, service, and labels; each incident is
//!   analyzed only once), new automatic findings (filtered by kind, minimum severity, and subject;
//!   each finding is investigated only once), manual.
//! - See [`AutomationRunStatus`] for run statuses; with no one to ask, questions, confirmations,
//!   and missing credentials cause the run to be marked as blocked without waiting.

mod findings;
mod incidents;

use std::collections::BTreeMap;

use async_trait::async_trait;
use domain::notify::connector::NotifyTargetType;
pub use findings::FindingFilter;
pub use incidents::{IncidentFacts, MERGE_WINDOW_MICROS};
use kernel::{Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::scope::Scope;

/// The maximum number of rules per organization.
pub const MAX_AUTOMATIONS_PER_ORG: usize = 50;
/// The maximum task prompt length in characters.
pub const MAX_PROMPT_CHARS: usize = 8_000;
/// The maximum number of notification channels a rule can send to.
pub const MAX_CHANNELS: usize = 5;
/// The maximum length in characters of `text` in the request body for an API trigger.
pub const MAX_INPUT_CHARS: usize = 8_000;
/// The default maximum run duration, configurable from 1 to 60 minutes.
pub const DEFAULT_TIMEOUT_SECS: u32 = 600;
pub const MIN_TIMEOUT_SECS: u32 = 60;
pub const MAX_TIMEOUT_SECS: u32 = 3_600;
/// The minimum interval between manual runs.
pub const MANUAL_RUN_INTERVAL_MICROS: i64 = 60_000_000;
/// The number of run records retained per rule; older records are deleted.
pub const KEPT_RUNS_PER_AUTOMATION: i64 = 100;

/// When to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationTrigger {
    /// Runs on a schedule in a time zone using a Cron expression (five fields: minute, hour, day,
    /// month, weekday).
    Schedule { cron: String, timezone: String },
    /// External systems trigger a run by calling the API with the rule's token; `text` in the
    /// request body provides context for this run.
    Api,
    /// Triggers on a new incident.
    Incident(IncidentFilter),
    /// Triggers on a problem newly found by automatic discovery.
    Finding(FindingFilter),
    /// Runs only manually.
    Manual,
}

impl AutomationTrigger {
    pub fn kind(&self) -> TriggerKind {
        match self {
            Self::Schedule { .. } => TriggerKind::Schedule,
            Self::Api => TriggerKind::Api,
            Self::Incident(_) => TriggerKind::Incident,
            Self::Finding(_) => TriggerKind::Finding,
            Self::Manual => TriggerKind::Manual,
        }
    }

    /// Incident or finding triggers need to read new items after the cursor.
    pub fn reads_events(&self) -> bool {
        matches!(self, Self::Incident(_) | Self::Finding(_))
    }
}

/// Specifies which new incidents trigger a run: empty fields impose no limit; all set fields must
/// match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentFilter {
    /// `info`, `warning`, `error`, `critical`.
    #[serde(default)]
    pub severities: Vec<String>,
    /// Alert rule IDs.
    #[serde(default)]
    pub rule_ids: Vec<String>,
    /// Affected services.
    #[serde(default)]
    pub services: Vec<String>,
    /// Incident labels whose values must match.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

/// How a run is triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    Schedule,
    Api,
    Incident,
    Finding,
    Manual,
}

impl TriggerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Api => "api",
            Self::Incident => "incident",
            Self::Finding => "finding",
            Self::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Schedule,
            Self::Api,
            Self::Incident,
            Self::Finding,
            Self::Manual,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == value)
    }
}

/// Where results are sent: a fixed address or group on a notification connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationChannel {
    pub connector_id: Id,
    pub target_type: NotifyTargetType,
    pub target: String,
}

/// What to do after a run ends.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationOutputs {
    /// Sends results to these channels.
    #[serde(default)]
    pub channels: Vec<AutomationChannel>,
    /// Incident-triggered runs write the conclusion as the incident's RCA.
    #[serde(default)]
    pub write_rca: bool,
}

/// The latest run, showing the rule's current state in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastAutomationRun {
    pub id: Id,
    pub at: TimestampMicros,
    pub status: AutomationRunStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    pub id: Id,
    pub org_id: Id,
    /// The session scope for each run; immutable after creation, so clone to change the scope.
    pub scope: Scope,
    pub name: String,
    pub description: String,
    /// Passed to Mole Agent as the question for each run.
    pub prompt: String,
    pub enabled: bool,
    pub trigger: AutomationTrigger,
    pub outputs: AutomationOutputs,
    /// The execution identity: the person who last saves the rule.
    pub run_as_user_id: Id,
    /// The interface language at save time, used for both answers and notifications.
    pub language: String,
    pub timeout_secs: u32,
    /// The built-in template used to create the rule.
    pub template: Option<String>,
    /// The API trigger token's prefix; empty when there is no token.
    /// The token itself is shown only once, when generated.
    pub api_token_prefix: Option<String>,
    /// The time of the next scheduled run.
    pub next_run_at: Option<TimestampMicros>,
    /// For incident and finding triggers, only incidents and findings created after this time count
    /// as new.
    pub incident_cursor: Option<TimestampMicros>,
    pub last_run: Option<LastAutomationRun>,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// The current stage of a run and how it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    /// Mole Agent is working.
    Running,
    /// The task is complete, and all results are delivered.
    Succeeded,
    /// The task is complete, but some notifications fail to send or the RCA fails to write.
    Partial,
    /// The task is incomplete: a model error, a timeout, or an execution identity that is no longer
    /// available.
    Failed,
    /// The run does not start: the budget is exhausted, the previous run is still in progress, or
    /// the same incident was just analyzed.
    Skipped,
    /// The task needs someone to answer, confirm, or provide credentials, but no one is available.
    Blocked,
    /// The run is stopped or interrupted.
    Abandoned,
}

impl AutomationRunStatus {
    pub const ALL: [Self; 7] = [
        Self::Running,
        Self::Succeeded,
        Self::Partial,
        Self::Failed,
        Self::Skipped,
        Self::Blocked,
        Self::Abandoned,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Blocked => "blocked",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }

    pub fn is_over(self) -> bool {
        self != Self::Running
    }
}

/// Reasons a run does not start or complete successfully: fixed codes displayed in the reader's
/// language on the page.
pub mod reasons {
    /// The organization's Mole Agent token budget for today is exhausted.
    pub const BUDGET_EXHAUSTED: &str = "budget_exhausted";
    /// The previous run is still in progress.
    pub const STILL_RUNNING: &str = "still_running";
    /// The same incident (or group of incidents) was just analyzed.
    pub const ALREADY_ANALYZED: &str = "already_analyzed";
    /// The execution identity is no longer available: the account is disabled, leaves the
    /// organization, or loses permissions.
    pub const RUN_AS_UNAVAILABLE: &str = "run_as_unavailable";
    /// No model service is available.
    pub const NO_MODEL: &str = "no_model";
    /// The rule's total time limit is exceeded.
    pub const TIMEOUT: &str = "timeout";
    /// A model or tool error occurs.
    pub const ERROR: &str = "error";
    /// Someone needs to answer a question.
    pub const QUESTION: &str = "question";
    /// Someone needs to confirm execution of a tool.
    pub const CONFIRMATION: &str = "confirmation";
    /// An MCP server requires the execution identity's own credentials, which are not yet saved.
    pub const CREDENTIAL: &str = "credential";
    /// Some notifications fail to send.
    pub const DELIVERY: &str = "delivery";
    /// The incident's RCA fails to write.
    pub const RCA: &str = "rca";
    /// Someone stops the run.
    pub const STOPPED: &str = "stopped";
    /// The server executing the run stops.
    pub const INTERRUPTED: &str = "interrupted";
}

/// A single run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRun {
    pub id: Id,
    pub org_id: Id,
    pub automation_id: Id,
    pub trigger: TriggerKind,
    pub status: AutomationRunStatus,
    /// Codes from [`reasons`], separated by commas when there are multiple.
    pub reason: Option<String>,
    /// The hidden session: open it to see the entire process.
    pub chat_id: Option<Id>,
    /// The Run for this answer turn.
    pub run_id: Option<Id>,
    /// The incident for an incident-triggered run.
    pub incident_id: Option<Id>,
    /// The key for a group of incidents (the incident fingerprint): analyzed only once within a
    /// time period.
    pub group_key: Option<String>,
    /// Context for this run: text from the API request or the incident summary.
    pub input: Option<String>,
    /// The beginning of the answer, shown in the list.
    pub summary: Option<String>,
    /// Notification and RCA results: `{"deliveries": [...], "rca": true}`.
    pub outputs: Value,
    /// The person who starts a manual run.
    pub started_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub finished_at: Option<TimestampMicros>,
}

/// How a run ends, written back to the run record.
#[derive(Debug, Clone, PartialEq)]
pub struct RunEnding {
    pub status: AutomationRunStatus,
    pub reason: Option<String>,
    pub summary: Option<String>,
    pub outputs: Value,
}

#[async_trait]
pub trait AutomationRepository: Send + Sync {
    async fn list(&self, org_id: &Id) -> Result<Vec<Automation>>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Automation>;
    /// Looks up by ID when the organization is unknown (API triggers carry only the rule ID), along
    /// with the token hash.
    async fn get_with_token(&self, id: &Id) -> Result<Option<(Automation, Option<String>)>>;
    async fn count(&self, org_id: &Id) -> Result<usize>;
    async fn create(&self, automation: Automation) -> Result<Automation>;
    async fn update(&self, automation: Automation) -> Result<Automation>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<bool>;
    async fn set_enabled(&self, org_id: &Id, id: &Id, enabled: bool) -> Result<Automation>;
    /// Replaces the API Token (hash and prefix); `None` revokes it.
    async fn set_api_token(
        &self,
        org_id: &Id,
        id: &Id,
        token: Option<(String, String)>,
    ) -> Result<()>;
    /// Claims scheduled rules due to run, earliest first.
    /// Claiming first moves the next run to `hold_until` so other nodes cannot claim it;
    /// [`Self::set_next_run`] writes back the actual next run once calculated. If the node
    /// disappears between these steps, the rule runs normally a little later.
    async fn claim_due(
        &self,
        now: TimestampMicros,
        hold_until: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<Automation>>;
    async fn set_next_run(&self, id: &Id, next: Option<TimestampMicros>) -> Result<()>;
    /// Enabled incident-triggered and finding-triggered rules across organizations.
    async fn incident_rules(&self) -> Result<Vec<Automation>>;
    /// Advances the incident (or finding) cursor from `from` to `to`; returns false if another
    /// node has already advanced it and handles this batch.
    async fn advance_incident_cursor(
        &self,
        id: &Id,
        from: Option<TimestampMicros>,
        to: TimestampMicros,
    ) -> Result<bool>;
    /// Whether the organization has an enabled incident-triggered rule that writes incident RCA.
    async fn writes_rca(&self, org_id: &Id) -> Result<bool>;

    async fn create_run(&self, run: AutomationRun) -> Result<AutomationRun>;
    async fn attach_run(&self, id: &Id, chat_id: &Id, run_id: &Id) -> Result<()>;
    async fn finish_run(&self, id: &Id, ending: &RunEnding) -> Result<Option<AutomationRun>>;
    async fn get_run(&self, org_id: &Id, id: &Id) -> Result<AutomationRun>;
    async fn run_for_agent_run(&self, run_id: &Id) -> Result<Option<AutomationRun>>;
    async fn list_runs(
        &self,
        org_id: &Id,
        automation_id: &Id,
        limit: i64,
    ) -> Result<Vec<AutomationRun>>;
    /// Run records still in progress across organizations, finalized here if their node disappears.
    async fn running_runs(&self, limit: i64) -> Result<Vec<AutomationRun>>;
    /// Whether this rule has analyzed this incident or the same group of incidents since `since`.
    async fn analyzed(
        &self,
        automation_id: &Id,
        incident_id: &Id,
        group_key: &str,
        since: TimestampMicros,
    ) -> Result<bool>;
    /// Keeps only the latest `keep` run records for each rule.
    async fn purge_runs(&self, keep: i64) -> Result<u64>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn triggers_read_back_with_their_kind() {
        let schedule = AutomationTrigger::Schedule {
            cron: "0 9 * * 1-5".into(),
            timezone: "Asia/Shanghai".into(),
        };
        let value = serde_json::to_value(&schedule).unwrap();
        assert_eq!(value["kind"], "schedule");
        assert_eq!(
            serde_json::from_value::<AutomationTrigger>(value).unwrap(),
            schedule
        );

        let incident: AutomationTrigger = serde_json::from_value(json!({
            "kind": "incident", "severities": ["critical"], "labels": {"env": "prod"}
        }))
        .unwrap();
        let AutomationTrigger::Incident(filter) = &incident else {
            panic!("an incident trigger");
        };
        assert_eq!(filter.severities, vec!["critical".to_string()]);
        assert!(filter.rule_ids.is_empty());
        assert_eq!(incident.kind(), TriggerKind::Incident);

        assert_eq!(
            serde_json::to_value(AutomationTrigger::Api).unwrap(),
            json!({"kind": "api"})
        );
    }

    #[test]
    fn statuses_and_trigger_kinds_round_trip() {
        for status in AutomationRunStatus::ALL {
            assert_eq!(AutomationRunStatus::parse(status.as_str()), Some(status));
        }
        assert!(!AutomationRunStatus::Running.is_over());
        assert!(AutomationRunStatus::Blocked.is_over());

        for kind in [
            TriggerKind::Schedule,
            TriggerKind::Api,
            TriggerKind::Incident,
            TriggerKind::Finding,
            TriggerKind::Manual,
        ] {
            assert_eq!(TriggerKind::parse(kind.as_str()), Some(kind));
        }
    }
}
