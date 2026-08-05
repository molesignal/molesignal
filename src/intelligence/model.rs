// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 控制面领域模型。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvestigationStatus {
    Draft,
    Pending,
    Running,
    WaitingForData,
    WaitingForApproval,
    VerifyingRecovery,
    Completed,
    PartiallyCompleted,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactStatus {
    Verified,
    Inference,
    Suggestion,
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisStatus {
    Proposed,
    Testing,
    Supported,
    InsufficientEvidence,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    L0,
    L1,
    L2,
    L3,
    L4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAccess {
    Blocked,
    Allowed,
}

impl RiskLevel {
    pub fn required_approvals(self) -> i32 {
        match self {
            Self::L0 | Self::L1 => 0,
            Self::L2 => 1,
            Self::L3 | Self::L4 => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
    Cancelled,
    Executed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Pending,
    Running,
    Succeeded,
    PartiallySucceeded,
    Failed,
    Cancelled,
    RolledBack,
    VerificationFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceSourceType {
    Logs,
    Metrics,
    Traces,
    Alerts,
    Profiles,
    Pipelines,
    Dashboard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntelligenceContext {
    pub source_type: IntelligenceSourceType,
    pub source_id: Option<String>,
    pub time_range: Value,
    #[serde(default)]
    pub filters: Value,
    pub service_name: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub alert_id: Option<String>,
    pub pipeline_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Investigation {
    pub id: Id,
    pub org_id: Id,
    pub created_by: Id,
    pub chat_id: Option<Id>,
    pub title: String,
    pub status: InvestigationStatus,
    pub context: Value,
    pub summary: Option<String>,
    pub confidence: Option<ConfidenceLevel>,
    pub current_step: Option<String>,
    pub started_at: Option<TimestampMicros>,
    pub completed_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationStep {
    pub id: Id,
    pub investigation_id: Id,
    pub org_id: Id,
    pub position: i32,
    pub title: String,
    pub status: StepStatus,
    pub tool_name: Option<String>,
    pub input: Value,
    pub output_summary: Option<String>,
    pub conclusion_impact: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<TimestampMicros>,
    pub ended_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationEvidence {
    pub id: Id,
    pub investigation_id: Id,
    pub step_id: Option<Id>,
    pub org_id: Id,
    pub kind: String,
    pub label: String,
    pub fact_status: FactStatus,
    pub source_ref: Value,
    pub query: Option<String>,
    pub parameters: Value,
    pub summary: String,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationHypothesis {
    pub id: Id,
    pub investigation_id: Id,
    pub org_id: Id,
    pub statement: String,
    pub confidence: ConfidenceLevel,
    pub status: HypothesisStatus,
    pub evidence_ids: Vec<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Automation {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub trigger: Value,
    pub input_context: Value,
    pub steps: Value,
    pub allowed_tools: Vec<String>,
    pub approval_policy: Value,
    pub output_actions: Value,
    pub failure_policy: Value,
    pub notification: Value,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Id,
    pub org_id: Id,
    pub investigation_id: Option<Id>,
    pub action: String,
    pub target: String,
    pub parameters: Value,
    pub reason: String,
    pub impact: String,
    pub risk: RiskLevel,
    pub status: ApprovalStatus,
    pub requested_by: Id,
    pub required_approvals: i32,
    pub reviews: Value,
    pub expires_at: Option<TimestampMicros>,
    pub decided_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub id: Id,
    pub org_id: Id,
    pub approval_request_id: Id,
    pub investigation_id: Option<Id>,
    pub action: String,
    pub target: String,
    pub parameters: Value,
    pub idempotency_key: String,
    pub requested_by: Id,
    pub approved_by: Vec<Id>,
    pub status: ExecutionStatus,
    pub output_summary: Option<String>,
    pub error: Option<String>,
    pub verification: Value,
    pub started_at: Option<TimestampMicros>,
    pub finished_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: Id,
    pub org_id: Id,
    pub chat_id: Option<Id>,
    pub investigation_id: Option<Id>,
    pub step_id: Option<Id>,
    pub tool_name: String,
    pub risk: RiskLevel,
    pub input: Value,
    pub output_summary: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub duration_ms: i64,
    pub called_by: Id,
    pub call_source: String,
    pub profile_id: Option<Id>,
    pub approval_id: Option<Id>,
    pub policy_decision: Value,
    pub audit_id: Option<Id>,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub description: String,
    pub model_provider_id: Option<Id>,
    pub model: Option<String>,
    pub allowed_tools: Vec<String>,
    pub data_scope: Value,
    pub risk_policy: Value,
    pub network_access: NetworkAccess,
    pub max_context_tokens: i32,
    pub max_investigation_secs: i32,
    pub max_tool_calls: i32,
    pub is_default: bool,
    pub enabled: bool,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationDetail {
    pub investigation: Investigation,
    pub steps: Vec<InvestigationStep>,
    pub evidence: Vec<InvestigationEvidence>,
    pub hypotheses: Vec<InvestigationHypothesis>,
}

#[async_trait]
pub trait IntelligenceRepository: Send + Sync {
    async fn list_investigations(&self, org_id: &Id) -> Result<Vec<Investigation>>;
    async fn get_investigation(&self, org_id: &Id, id: &Id) -> Result<InvestigationDetail>;
    async fn create_investigation(&self, item: Investigation) -> Result<Investigation>;
    async fn update_investigation(&self, item: Investigation) -> Result<Investigation>;
    async fn append_step(&self, item: InvestigationStep) -> Result<InvestigationStep>;
    async fn append_evidence(&self, item: InvestigationEvidence) -> Result<InvestigationEvidence>;
    async fn upsert_hypothesis(
        &self,
        item: InvestigationHypothesis,
    ) -> Result<InvestigationHypothesis>;

    async fn list_automations(&self, org_id: &Id) -> Result<Vec<Automation>>;
    async fn get_automation(&self, org_id: &Id, id: &Id) -> Result<Automation>;
    async fn create_automation(&self, item: Automation) -> Result<Automation>;
    async fn update_automation(&self, item: Automation) -> Result<Automation>;

    async fn list_approvals(&self, org_id: &Id) -> Result<Vec<ApprovalRequest>>;
    async fn get_approval(&self, org_id: &Id, id: &Id) -> Result<ApprovalRequest>;
    async fn create_approval(&self, item: ApprovalRequest) -> Result<ApprovalRequest>;
    async fn review_approval(
        &self,
        org_id: &Id,
        id: &Id,
        reviewer: &Id,
        approve: bool,
        comment: &str,
        now: TimestampMicros,
    ) -> Result<ApprovalRequest>;
    async fn mark_approval_executed(
        &self,
        org_id: &Id,
        id: &Id,
        now: TimestampMicros,
    ) -> Result<ApprovalRequest>;

    async fn list_executions(&self, org_id: &Id) -> Result<Vec<Execution>>;
    async fn get_execution(&self, org_id: &Id, id: &Id) -> Result<Execution>;
    async fn find_execution_by_key(&self, org_id: &Id, key: &str) -> Result<Option<Execution>>;
    async fn create_execution(&self, item: Execution) -> Result<Execution>;
    async fn update_execution(&self, item: Execution) -> Result<Execution>;

    async fn record_tool_call(&self, item: ToolCallRecord) -> Result<ToolCallRecord>;
    async fn list_tool_calls(
        &self,
        org_id: &Id,
        tool_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ToolCallRecord>>;

    async fn list_profiles(&self, org_id: &Id) -> Result<Vec<AgentProfile>>;
    async fn get_profile(&self, org_id: &Id, id: &Id) -> Result<AgentProfile>;
    async fn create_profile(&self, item: AgentProfile) -> Result<AgentProfile>;
    async fn update_profile(&self, item: AgentProfile) -> Result<AgentProfile>;
}
