// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One response turn equals one Run.
//!
//! Sending a message only queues the Run; a node executes it in the background and records progress
//! as events with increasing sequence numbers ([`RunEvent`]). The browser resumes reading by
//! sequence number from any node, so navigating away or refreshing does not interrupt the response.
//! Stop requests, the requester's answers, and confirmations are recorded on the Run and picked up
//! by the executing node. Runs in the same session execute one after another in order.
//!
//! - The execution identity is recorded as [`RunPrincipal`], which is used to recheck the account
//!   and permissions at execution time.
//! - A running Run holds a lease that the node renews periodically; an expired lease means the node
//!   is gone, and the Run is marked as [`RunStatus::Interrupted`].

mod context;
mod events;
mod signals;
mod transcript;

use std::time::Duration;

use async_trait::async_trait;
pub use context::{
    COMPACTION_INSTRUCTION, COMPACTION_KEEP_TURNS, compaction_due, compaction_split,
    estimate_tokens, language_instruction, summary_text,
};
use domain::iam::{IamContext, IamScope};
pub use events::{
    NewRunEvent, RunEvent, RunEventKind, TEXT_FLUSH_BYTES, TEXT_FLUSH_MILLIS, TextBuffer,
};
use kernel::{Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use signals::{RunSignalHub, RunSignals};
use tool_runtime::RiskLevel;
pub use transcript::{
    MAX_INLINE_RESULT_BYTES, TranscriptBuilder, TranscriptCall, TranscriptStep, inline_result,
    transcript_messages,
};

use crate::usage::Cost;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    /// Waits for the requester to answer a question or save their credentials for an MCP server.
    WaitingInput,
    /// Waits for the requester to confirm running a tool.
    WaitingApproval,
    Succeeded,
    Failed,
    Cancelled,
    /// The executing node is gone.
    Interrupted,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingInput => "waiting_input",
            Self::WaitingApproval => "waiting_approval",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::Running,
            Self::WaitingInput,
            Self::WaitingApproval,
            Self::Succeeded,
            Self::Failed,
            Self::Cancelled,
            Self::Interrupted,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    /// Runs on a node, including while waiting for answers and confirmations.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Running | Self::WaitingInput | Self::WaitingApproval
        )
    }
}

/// Where the Run originates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSource {
    Chat,
    Automation,
    Incident,
    Api,
    Im,
    /// A drill: a resolved incident investigated again, graded against its known root cause.
    Drill,
}

impl RunSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Automation => "automation",
            Self::Incident => "incident",
            Self::Api => "api",
            Self::Im => "im",
            Self::Drill => "drill",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Chat,
            Self::Automation,
            Self::Incident,
            Self::Api,
            Self::Im,
            Self::Drill,
        ]
        .into_iter()
        .find(|source| source.as_str() == value)
    }
}

/// The identity used for execution.
/// Only the identity is recorded, not permissions: they are resolved again at execution time, so
/// permissions revoked in the meantime are not used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPrincipal {
    pub user_id: Id,
    pub scope: IamScope,
    #[serde(default)]
    pub credential_token_id: Option<Id>,
    #[serde(default)]
    pub credential_application_id: Option<String>,
    #[serde(default)]
    pub credential_service_account_id: Option<Id>,
}

impl RunPrincipal {
    pub fn of(ctx: &IamContext) -> Self {
        Self {
            user_id: ctx.user_id.clone(),
            scope: ctx.scope,
            credential_token_id: ctx.credential_token_id.clone(),
            credential_application_id: ctx.credential_application_id.clone(),
            credential_service_account_id: ctx.credential_service_account_id.clone(),
        }
    }

    /// An identity with unresolved permissions, passed to IAM to complete.
    pub fn unresolved_context(&self, org_id: &Id) -> IamContext {
        IamContext {
            user_id: self.user_id.clone(),
            org_id: org_id.clone(),
            display_role: String::new(),
            roles: Vec::new(),
            credential_token_id: self.credential_token_id.clone(),
            credential_application_id: self.credential_application_id.clone(),
            credential_service_account_id: self.credential_service_account_id.clone(),
            scope: self.scope,
            permissions: Default::default(),
            policy_version: 0,
        }
    }
}

/// The way a question is asked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    Single,
    Multiple,
    #[default]
    Text,
}

/// What is awaited from the requester.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Pending {
    Question {
        call_id: String,
        question: String,
        #[serde(default)]
        kind: QuestionKind,
        #[serde(default)]
        options: Vec<String>,
    },
    Confirmation {
        call_id: String,
        tool: String,
        arguments: Value,
        risk: RiskLevel,
    },
    /// `Authorize to continue`: an MCP server requiring member credentials lacks the requester's
    /// credentials or rejects the saved ones. This tool call runs again after the credentials are
    /// saved.
    Authorization {
        call_id: String,
        /// The tool that uses these credentials.
        tool: String,
        server_id: String,
        server: String,
        /// The request header that holds the credentials, in lowercase.
        header: String,
        /// Prefixes the value with `Bearer `.
        #[serde(default)]
        bearer: bool,
        /// Administrator-written instructions on where to get credentials and which permissions
        /// are needed.
        #[serde(default)]
        instructions: String,
        /// The remote server rejects the saved credentials.
        #[serde(default)]
        rejected: bool,
    },
}

impl Pending {
    pub fn call_id(&self) -> &str {
        match self {
            Self::Question { call_id, .. }
            | Self::Confirmation { call_id, .. }
            | Self::Authorization { call_id, .. } => call_id,
        }
    }

    pub fn status(&self) -> RunStatus {
        match self {
            Self::Question { .. } | Self::Authorization { .. } => RunStatus::WaitingInput,
            Self::Confirmation { .. } => RunStatus::WaitingApproval,
        }
    }
}

/// The requester's answer or decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RunResponse {
    Answer {
        call_id: String,
        /// The selected options.
        #[serde(default)]
        choices: Vec<String>,
        /// The written answer.
        #[serde(default)]
        text: String,
        /// Who answers: in a team session, this is not necessarily the person who initiates this
        /// turn.
        responder: Id,
    },
    Decision {
        call_id: String,
        approved: bool,
        responder: Id,
    },
    /// The requester saves the credentials and execution continues, or skips this server.
    Authorization {
        call_id: String,
        authorized: bool,
        responder: Id,
    },
}

impl RunResponse {
    pub fn call_id(&self) -> &str {
        match self {
            Self::Answer { call_id, .. }
            | Self::Decision { call_id, .. }
            | Self::Authorization { call_id, .. } => call_id,
        }
    }

    /// Matches the pending question, confirmation, or authorization.
    pub fn answers(&self, pending: &Pending) -> bool {
        self.call_id() == pending.call_id()
            && matches!(
                (self, pending),
                (Self::Answer { .. }, Pending::Question { .. })
                    | (Self::Decision { .. }, Pending::Confirmation { .. })
                    | (Self::Authorization { .. }, Pending::Authorization { .. })
            )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: Id,
    pub org_id: Id,
    pub chat_id: Id,
    pub source: RunSource,
    pub status: RunStatus,
    pub principal: RunPrincipal,
    /// The request used to execute this turn: mode, time, model, Agent configuration, instructions
    /// from commands, etc.
    pub request: Value,
    pub user_message_id: Option<Id>,
    pub assistant_message_id: Option<Id>,
    /// The executing node.
    pub node: Option<String>,
    pub lease_expires_at: Option<TimestampMicros>,
    pub cancel_requested: bool,
    pub pending: Option<Pending>,
    pub response: Option<RunResponse>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// Recorded in the currency selected for the model service; empty if no unit price is
    /// available.
    pub cost: Option<Cost>,
    pub error: Option<String>,
    /// The sequence number of the last event.
    pub last_seq: i64,
    pub created_at: TimestampMicros,
    pub started_at: Option<TimestampMicros>,
    pub finished_at: Option<TimestampMicros>,
    pub updated_at: TimestampMicros,
}

impl Run {
    /// The person who initiates this turn.
    pub fn initiator(&self) -> &Id {
        &self.principal.user_id
    }

    pub fn queued(
        org_id: Id,
        chat_id: Id,
        source: RunSource,
        principal: RunPrincipal,
        request: Value,
        user_message_id: Option<Id>,
    ) -> Self {
        let now = TimestampMicros::now();

        Self {
            id: Id::new(),
            org_id,
            chat_id,
            source,
            status: RunStatus::Queued,
            principal,
            request,
            user_message_id,
            assistant_message_id: None,
            node: None,
            lease_expires_at: None,
            cancel_requested: false,
            pending: None,
            response: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            cost: None,
            error: None,
            last_seq: 0,
            created_at: now,
            started_at: None,
            finished_at: None,
            updated_at: now,
        }
    }
}

/// How a response turn ends.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub status: RunStatus,
    pub assistant_message_id: Option<Id>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost: Option<Cost>,
    pub error: Option<String>,
}

#[async_trait]
pub trait RunRepository: Send + Sync {
    /// Adds the Run to the queue and wakes nodes waiting to claim it.
    async fn create(&self, run: Run) -> Result<Run>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Run>;
    /// The most recent Runs in a session, newest first.
    async fn for_chat(&self, org_id: &Id, chat_id: &Id, limit: i64) -> Result<Vec<Run>>;
    /// Claims the earliest runnable Run: its turn comes only after all earlier Runs in the same
    /// session finish.
    async fn claim_next(&self, node: &str, lease_until: TimestampMicros) -> Result<Option<Run>>;
    /// Waits up to `timeout` until a Run may be available to claim, waking when a Run is queued
    /// or finishes.
    async fn wait_for_pending(&self, timeout: Duration) -> bool;
    /// Subscribes to Run notifications on this node: new events, stops, and answers.
    fn signals(&self) -> RunSignals;
    /// Renews the lease; returns false if the Run no longer belongs to this node or has finished.
    async fn renew(&self, id: &Id, node: &str, lease_until: TimestampMicros) -> Result<bool>;
    /// Starts waiting for the requester: records what is awaited and sets the corresponding
    /// waiting status.
    async fn wait_for(&self, id: &Id, pending: &Pending) -> Result<()>;
    /// The requester's answer or decision; reports a conflict if it does not match what is awaited.
    async fn respond(&self, org_id: &Id, id: &Id, response: &RunResponse) -> Result<Run>;
    /// Takes the answer and returns the status to running; returns `None` if there is no answer
    /// yet.
    async fn take_response(&self, id: &Id) -> Result<Option<RunResponse>>;
    /// Stops waiting after too long: clears what is awaited and returns the status to running;
    /// later answers are rejected.
    async fn stop_waiting(&self, id: &Id) -> Result<()>;
    /// Stops the Run: ends a queued Run immediately, or records a request for the executing node
    /// to stop a running Run.
    async fn request_cancel(&self, org_id: &Id, id: &Id) -> Result<Run>;
    async fn cancel_requested(&self, id: &Id) -> Result<bool>;
    /// Appends events, returns the last sequence number, and notifies connections that are reading
    /// further events. Reports a conflict if the Run has already finished.
    async fn append_events(&self, org_id: &Id, run_id: &Id, events: &[NewRunEvent]) -> Result<i64>;
    /// Finishes the Run by appending the final events (ending with [`RunEventKind::Done`]) and
    /// recording the outcome. If the Run has already finished (for example, marked as interrupted),
    /// writes nothing and returns false.
    async fn finish(&self, run: &Run, outcome: &RunOutcome, events: &[NewRunEvent])
    -> Result<bool>;
    async fn events_after(&self, run_id: &Id, after: i64, limit: i64) -> Result<Vec<RunEvent>>;
    /// Marks Runs with expired leases as interrupted, adds a completion event to each, and
    /// returns these Runs.
    async fn expire_leases(&self, now: TimestampMicros) -> Result<Vec<Run>>;
    /// Deletes events earlier than `before` and returns the number deleted.
    async fn purge_events(&self, before: TimestampMicros) -> Result<u64>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip_and_say_whether_they_are_over() {
        for status in [
            RunStatus::Queued,
            RunStatus::Running,
            RunStatus::WaitingInput,
            RunStatus::WaitingApproval,
            RunStatus::Succeeded,
            RunStatus::Failed,
            RunStatus::Cancelled,
            RunStatus::Interrupted,
        ] {
            assert_eq!(RunStatus::parse(status.as_str()), Some(status));
            assert!(!(status.is_terminal() && status.is_live()));
        }

        assert!(RunStatus::Interrupted.is_terminal());
        assert!(RunStatus::WaitingApproval.is_live());
        assert!(!RunStatus::Queued.is_live());
    }

    #[test]
    fn a_response_must_answer_what_is_waited_for() {
        let question = Pending::Question {
            call_id: "call-1".into(),
            question: "Which environment?".into(),
            kind: QuestionKind::Single,
            options: vec!["staging".into(), "production".into()],
        };

        let answer = RunResponse::Answer {
            call_id: "call-1".into(),
            choices: vec!["production".into()],
            text: String::new(),
            responder: Id::from_string("u1"),
        };

        let decision = RunResponse::Decision {
            call_id: "call-1".into(),
            approved: true,
            responder: Id::from_string("u1"),
        };

        assert!(answer.answers(&question));
        assert!(!decision.answers(&question));
        assert_eq!(question.status(), RunStatus::WaitingInput);

        let other = RunResponse::Answer {
            call_id: "call-2".into(),
            choices: Vec::new(),
            text: "yes".into(),
            responder: Id::from_string("u1"),
        };

        assert!(!other.answers(&question));
    }

    #[test]
    fn an_authorization_waits_for_input_and_takes_only_an_authorization() {
        let pending = Pending::Authorization {
            call_id: "call-1".into(),
            tool: "linear_search_issues".into(),
            server_id: "server-1".into(),
            server: "Linear".into(),
            header: "authorization".into(),
            bearer: true,
            instructions: String::new(),
            rejected: false,
        };

        let authorized = RunResponse::Authorization {
            call_id: "call-1".into(),
            authorized: true,
            responder: Id::from_string("u1"),
        };

        let decision = RunResponse::Decision {
            call_id: "call-1".into(),
            approved: true,
            responder: Id::from_string("u1"),
        };

        assert!(authorized.answers(&pending));
        assert!(!decision.answers(&pending));
        assert_eq!(pending.status(), RunStatus::WaitingInput);

        // The form stored on the Run: older records without optional fields can still be read.
        let stored = serde_json::json!({
            "type": "authorization", "callId": "call-1", "tool": "t",
            "serverId": "s", "server": "S", "header": "x-api-key",
        });
        let read: Pending = serde_json::from_value(stored).unwrap();
        assert!(matches!(
            read,
            Pending::Authorization {
                bearer: false,
                rejected: false,
                ..
            }
        ));
    }

    #[test]
    fn the_principal_keeps_the_credential_it_came_with() {
        let principal = RunPrincipal {
            user_id: Id::from_string("u1"),
            scope: IamScope::ApiToken,
            credential_token_id: Some(Id::from_string("token-1")),
            credential_application_id: None,
            credential_service_account_id: None,
        };

        let context = principal.unresolved_context(&Id::from_string("org-1"));

        assert_eq!(RunPrincipal::of(&context), principal);
        assert!(context.permissions.is_empty());
    }
}
