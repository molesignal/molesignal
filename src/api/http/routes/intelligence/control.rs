// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 调查、自动化、审批、执行与 Agent Profile API。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::{
    api::{
        AppState,
        http::{middleware::Permission, routes::activity_audit},
    },
    app::iam::IamContext,
    domain::{alerting::incident::IncidentStatus, iam::permission},
    intelligence::{
        FEATURE,
        model::{
            AgentProfile, ApprovalRequest, ApprovalStatus, Automation, ConfidenceLevel, Execution,
            ExecutionStatus, FactStatus, HypothesisStatus, Investigation, InvestigationEvidence,
            InvestigationHypothesis, InvestigationStatus, InvestigationStep, NetworkAccess,
            RiskLevel, StepStatus,
        },
        tool_control::ToolExecutionMode,
        tools::builtin_tools,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod dashboard_operation;

const DEFAULT_APPROVAL_TTL_MICROS: i64 = 60 * 60 * 1_000_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/intelligence/overview", get(overview))
        .route(
            "/intelligence/investigations",
            get(list_investigations).post(create_investigation),
        )
        .route(
            "/intelligence/investigations/{id}",
            get(get_investigation).put(update_investigation),
        )
        .route(
            "/intelligence/investigations/{id}/steps",
            post(append_investigation_step),
        )
        .route(
            "/intelligence/investigations/{id}/evidence",
            post(append_investigation_evidence),
        )
        .route(
            "/intelligence/investigations/{id}/hypotheses",
            post(upsert_investigation_hypothesis),
        )
        .route(
            "/intelligence/automations",
            get(list_automations).post(create_automation),
        )
        .route(
            "/intelligence/automations/{id}",
            get(get_automation).put(update_automation),
        )
        .route(
            "/intelligence/automations/{id}/dry-run",
            post(dry_run_automation),
        )
        .route(
            "/intelligence/approvals",
            get(list_approvals).post(create_approval),
        )
        .route("/intelligence/approvals/{id}", get(get_approval))
        .route("/intelligence/approvals/{id}/review", post(review_approval))
        .route(
            "/intelligence/approvals/{id}/execute",
            post(execute_approval),
        )
        .route("/intelligence/executions", get(list_executions))
        .route("/intelligence/executions/{id}", get(get_execution))
        .route(
            "/intelligence/settings/agent-profiles",
            get(list_profiles).post(create_profile),
        )
        .route(
            "/intelligence/settings/agent-profiles/{id}",
            get(get_profile).put(update_profile),
        )
}

fn require_license(state: &AppState) -> Result<()> {
    if !state.platform.license.has_feature(FEATURE) {
        return Err(Error::forbidden(format!("{FEATURE} feature not licensed")));
    }
    Ok(())
}

#[permission("intelligence.use")]
async fn overview(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let investigations = state
        .intelligence
        .repository
        .list_investigations(&ctx.org_id)
        .await?;
    let approvals = state
        .intelligence
        .repository
        .list_approvals(&ctx.org_id)
        .await?;
    let executions = state
        .intelligence
        .repository
        .list_executions(&ctx.org_id)
        .await?;
    let automations = state
        .intelligence
        .repository
        .list_automations(&ctx.org_id)
        .await?;
    Ok(Json(json!({
        "active_investigations": investigations.iter().filter(|item| matches!(
            item.status,
            InvestigationStatus::Pending
                | InvestigationStatus::Running
                | InvestigationStatus::WaitingForData
                | InvestigationStatus::WaitingForApproval
                | InvestigationStatus::VerifyingRecovery
        )).count(),
        "pending_approvals": approvals.iter().filter(|item| item.status == ApprovalStatus::Pending).count(),
        "recent_completed": investigations.iter().filter(|item| item.status == InvestigationStatus::Completed).count(),
        "automation_runs": executions.iter().filter(|item| item.investigation_id.is_none()).count(),
        "enabled_automations": automations.iter().filter(|item| item.enabled).count(),
    })))
}

#[derive(Debug, Deserialize)]
struct CreateInvestigationRequest {
    title: String,
    #[serde(default)]
    chat_id: Option<Id>,
    #[serde(default)]
    context: Value,
    #[serde(default)]
    steps: Vec<String>,
}

#[permission("intelligence.use")]
async fn list_investigations(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    Ok(Json(json!({
        "investigations": state.intelligence.repository.list_investigations(&ctx.org_id).await?
    })))
}

#[permission("intelligence.use")]
async fn create_investigation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<CreateInvestigationRequest>,
) -> Result<Json<Investigation>> {
    require_license(&state)?;
    if request.title.trim().is_empty() {
        return Err(Error::invalid("investigation title cannot be empty"));
    }
    let now = TimestampMicros::now();
    let item = Investigation {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        created_by: ctx.user_id.clone(),
        chat_id: request.chat_id,
        title: request.title,
        status: InvestigationStatus::Draft,
        context: request.context,
        summary: None,
        confidence: None,
        current_step: None,
        started_at: None,
        completed_at: None,
        created_at: now,
        updated_at: now,
    };
    let saved = state
        .intelligence
        .repository
        .create_investigation(item)
        .await?;
    for (position, title) in request.steps.into_iter().enumerate() {
        if title.trim().is_empty() {
            continue;
        }
        state
            .intelligence
            .repository
            .append_step(InvestigationStep {
                id: Id::new(),
                investigation_id: saved.id.clone(),
                org_id: ctx.org_id.clone(),
                position: i32::try_from(position).unwrap_or(i32::MAX),
                title,
                status: StepStatus::Pending,
                tool_name: None,
                input: json!({}),
                output_summary: None,
                conclusion_impact: None,
                error: None,
                started_at: None,
                ended_at: None,
                created_at: now,
            })
            .await?;
    }
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.investigation.created",
        "intelligence_investigation",
        &saved.id.0,
        json!({"title": saved.title}),
    )
    .await;
    Ok(Json(saved))
}

#[permission("intelligence.use")]
async fn get_investigation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    Ok(Json(
        serde_json::to_value(
            state
                .intelligence
                .repository
                .get_investigation(&ctx.org_id, &Id(id))
                .await?,
        )
        .unwrap_or(Value::Null),
    ))
}

#[derive(Debug, Deserialize)]
struct UpdateInvestigationRequest {
    title: Option<String>,
    status: Option<InvestigationStatus>,
    #[serde(default, deserialize_with = "deserialize_present_option")]
    summary: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_present_option")]
    confidence: Option<Option<ConfidenceLevel>>,
    #[serde(default, deserialize_with = "deserialize_present_option")]
    current_step: Option<Option<String>>,
    #[serde(default)]
    context: Option<Value>,
}

fn deserialize_present_option<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[permission("intelligence.use")]
async fn update_investigation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<UpdateInvestigationRequest>,
) -> Result<Json<Investigation>> {
    require_license(&state)?;
    let mut item = state
        .intelligence
        .repository
        .get_investigation(&ctx.org_id, &Id(id))
        .await?
        .investigation;
    if item.created_by != ctx.user_id {
        Permission::require_key(&ctx, "intelligence.manage")?;
    }
    if let Some(title) = request.title {
        if title.trim().is_empty() {
            return Err(Error::invalid("investigation title cannot be empty"));
        }
        item.title = title;
    }
    if let Some(status) = request.status {
        item.status = status;
    }
    if let Some(summary) = request.summary {
        item.summary = summary;
    }
    if let Some(confidence) = request.confidence {
        item.confidence = confidence;
    }
    if let Some(current_step) = request.current_step {
        item.current_step = current_step;
    }
    if let Some(context) = request.context {
        item.context = context;
    }
    let now = TimestampMicros::now();
    if item.status == InvestigationStatus::Running && item.started_at.is_none() {
        item.started_at = Some(now);
    }
    if matches!(
        item.status,
        InvestigationStatus::Completed
            | InvestigationStatus::PartiallyCompleted
            | InvestigationStatus::Failed
            | InvestigationStatus::Cancelled
    ) {
        item.completed_at = Some(now);
    }
    item.updated_at = now;
    Ok(Json(
        state
            .intelligence
            .repository
            .update_investigation(item)
            .await?,
    ))
}

#[derive(Debug, Deserialize)]
struct AppendStepRequest {
    position: i32,
    title: String,
    #[serde(default)]
    status: Option<StepStatus>,
    tool_name: Option<String>,
    #[serde(default)]
    input: Value,
    output_summary: Option<String>,
    conclusion_impact: Option<String>,
    error: Option<String>,
    started_at_micros: Option<i64>,
    ended_at_micros: Option<i64>,
}

#[permission("intelligence.use")]
async fn append_investigation_step(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<AppendStepRequest>,
) -> Result<Json<InvestigationStep>> {
    require_license(&state)?;
    let investigation_id = Id(id);
    state
        .intelligence
        .repository
        .get_investigation(&ctx.org_id, &investigation_id)
        .await?;
    if let Some(tool) = request.tool_name.as_deref()
        && !builtin_tools()
            .iter()
            .any(|registered| registered.name == tool)
    {
        return Err(Error::invalid(format!("tool `{tool}` is not registered")));
    }
    let item = InvestigationStep {
        id: Id::new(),
        investigation_id,
        org_id: ctx.org_id,
        position: request.position,
        title: request.title,
        status: request.status.unwrap_or(StepStatus::Pending),
        tool_name: request.tool_name,
        input: request.input,
        output_summary: request.output_summary,
        conclusion_impact: request.conclusion_impact,
        error: request.error,
        started_at: request.started_at_micros.map(TimestampMicros),
        ended_at: request.ended_at_micros.map(TimestampMicros),
        created_at: TimestampMicros::now(),
    };
    Ok(Json(state.intelligence.repository.append_step(item).await?))
}

#[derive(Debug, Deserialize)]
struct AppendEvidenceRequest {
    step_id: Option<Id>,
    kind: String,
    label: String,
    fact_status: FactStatus,
    #[serde(default)]
    source_ref: Value,
    query: Option<String>,
    #[serde(default)]
    parameters: Value,
    summary: String,
}

#[permission("intelligence.use")]
async fn append_investigation_evidence(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<AppendEvidenceRequest>,
) -> Result<Json<InvestigationEvidence>> {
    require_license(&state)?;
    let investigation_id = Id(id);
    state
        .intelligence
        .repository
        .get_investigation(&ctx.org_id, &investigation_id)
        .await?;
    let item = InvestigationEvidence {
        id: Id::new(),
        investigation_id,
        step_id: request.step_id,
        org_id: ctx.org_id,
        kind: request.kind,
        label: request.label,
        fact_status: request.fact_status,
        source_ref: request.source_ref,
        query: request.query,
        parameters: request.parameters,
        summary: request.summary,
        created_at: TimestampMicros::now(),
    };
    Ok(Json(
        state.intelligence.repository.append_evidence(item).await?,
    ))
}

#[derive(Debug, Deserialize)]
struct UpsertHypothesisRequest {
    id: Option<Id>,
    statement: String,
    confidence: ConfidenceLevel,
    status: HypothesisStatus,
    #[serde(default)]
    evidence_ids: Vec<Id>,
}

#[permission("intelligence.use")]
async fn upsert_investigation_hypothesis(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<UpsertHypothesisRequest>,
) -> Result<Json<InvestigationHypothesis>> {
    require_license(&state)?;
    let investigation_id = Id(id);
    state
        .intelligence
        .repository
        .get_investigation(&ctx.org_id, &investigation_id)
        .await?;
    let now = TimestampMicros::now();
    let item = InvestigationHypothesis {
        id: request.id.unwrap_or_else(Id::new),
        investigation_id,
        org_id: ctx.org_id,
        statement: request.statement,
        confidence: request.confidence,
        status: request.status,
        evidence_ids: request.evidence_ids,
        created_at: now,
        updated_at: now,
    };
    Ok(Json(
        state
            .intelligence
            .repository
            .upsert_hypothesis(item)
            .await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
struct AutomationRequest {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_true")]
    enabled: bool,
    trigger: Value,
    #[serde(default)]
    input_context: Value,
    #[serde(default)]
    steps: Value,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default)]
    approval_policy: Value,
    #[serde(default)]
    output_actions: Value,
    #[serde(default)]
    failure_policy: Value,
    #[serde(default)]
    notification: Value,
}

fn default_true() -> bool {
    true
}

async fn validate_allowed_tools(state: &AppState, org_id: &Id, tools: &[String]) -> Result<()> {
    let mut registered = builtin_tools()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<std::collections::HashSet<_>>();
    registered.extend(
        state
            .intelligence
            .tool_control
            .list_mcp_tools(org_id, None)
            .await?
            .into_iter()
            .map(|tool| tool.name),
    );
    for tool in tools {
        if !registered.contains(tool) {
            return Err(Error::invalid(format!("tool `{tool}` is not registered")));
        }
    }
    Ok(())
}

#[permission("intelligence.use")]
async fn list_automations(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    Ok(Json(json!({
        "automations": state.intelligence.repository.list_automations(&ctx.org_id).await?
    })))
}

#[permission("intelligence.manage")]
async fn create_automation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<AutomationRequest>,
) -> Result<Json<Automation>> {
    require_license(&state)?;
    validate_allowed_tools(&state, &ctx.org_id, &request.allowed_tools).await?;
    let now = TimestampMicros::now();
    let item = Automation {
        id: Id::new(),
        org_id: ctx.org_id,
        name: request.name,
        description: request.description,
        enabled: request.enabled,
        trigger: request.trigger,
        input_context: request.input_context,
        steps: request.steps,
        allowed_tools: request.allowed_tools,
        approval_policy: request.approval_policy,
        output_actions: request.output_actions,
        failure_policy: request.failure_policy,
        notification: request.notification,
        created_by: ctx.user_id,
        created_at: now,
        updated_at: now,
    };
    Ok(Json(
        state
            .intelligence
            .repository
            .create_automation(item)
            .await?,
    ))
}

#[permission("intelligence.use")]
async fn get_automation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Automation>> {
    require_license(&state)?;
    Ok(Json(
        state
            .intelligence
            .repository
            .get_automation(&ctx.org_id, &Id(id))
            .await?,
    ))
}

#[permission("intelligence.manage")]
async fn update_automation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<AutomationRequest>,
) -> Result<Json<Automation>> {
    require_license(&state)?;
    validate_allowed_tools(&state, &ctx.org_id, &request.allowed_tools).await?;
    let existing = state
        .intelligence
        .repository
        .get_automation(&ctx.org_id, &Id(id))
        .await?;
    let item = Automation {
        name: request.name,
        description: request.description,
        enabled: request.enabled,
        trigger: request.trigger,
        input_context: request.input_context,
        steps: request.steps,
        allowed_tools: request.allowed_tools,
        approval_policy: request.approval_policy,
        output_actions: request.output_actions,
        failure_policy: request.failure_policy,
        notification: request.notification,
        updated_at: TimestampMicros::now(),
        ..existing
    };
    Ok(Json(
        state
            .intelligence
            .repository
            .update_automation(item)
            .await?,
    ))
}

#[permission("intelligence.use")]
async fn dry_run_automation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(event): Json<Value>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let automation = state
        .intelligence
        .repository
        .get_automation(&ctx.org_id, &Id(id))
        .await?;
    validate_allowed_tools(&state, &ctx.org_id, &automation.allowed_tools).await?;
    let actions = automation
        .output_actions
        .as_array()
        .cloned()
        .unwrap_or_default();
    let approvals = actions
        .iter()
        .filter_map(|action| action["action"].as_str())
        .filter_map(|action| operation_policy(action).ok().map(|policy| (action, policy)))
        .map(|(action, (risk, _))| {
            json!({
                "action": action,
                "risk": risk,
                "requires_approval": risk.required_approvals() > 0,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "dry_run": true,
        "matched": automation.enabled,
        "event": event,
        "resolved_tools": automation.allowed_tools,
        "steps": automation.steps,
        "operations": approvals,
        "side_effects": false,
    })))
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateApprovalRequest {
    pub investigation_id: Option<Id>,
    pub action: String,
    pub target: String,
    #[serde(default)]
    pub parameters: Value,
    pub reason: String,
    pub impact: String,
    pub expires_at_micros: Option<i64>,
    /// Internal policy resolution. HTTP callers cannot set reviewer counts.
    #[serde(skip)]
    pub required_approvals_override: Option<i32>,
}

pub(crate) fn operation_policy(action: &str) -> Result<(RiskLevel, &'static str)> {
    match action {
        "acknowledge_alert" | "resolve_alert" => Ok((RiskLevel::L2, "alerts.acknowledge")),
        "create_dashboard" => Ok((RiskLevel::L1, "dashboards.create")),
        other => Err(Error::invalid(format!(
            "operation `{other}` is not registered"
        ))),
    }
}

pub(crate) fn dashboard_required_approvals(mode: ToolExecutionMode) -> Result<i32> {
    match mode {
        // Dashboard creation has a Confirmation hard floor. Automatic therefore still
        // creates an approved proposal that must be explicitly executed.
        ToolExecutionMode::Automatic | ToolExecutionMode::Confirmation => Ok(0),
        ToolExecutionMode::SingleApproval => Ok(1),
        ToolExecutionMode::DualApproval => Ok(2),
        ToolExecutionMode::Disabled => Err(Error::forbidden(
            "Dashboard creation is disabled by the active operation policy",
        )),
    }
}

pub(crate) async fn create_agent_approval(
    state: &AppState,
    ctx: &IamContext,
    request: CreateApprovalRequest,
) -> Result<ApprovalRequest> {
    let (risk, _) = operation_policy(&request.action)?;
    if request.target.trim().is_empty() {
        return Err(Error::invalid("operation target cannot be empty"));
    }
    let dashboard_draft_expiry = if request.action == "create_dashboard" {
        let expected_hash = request
            .parameters
            .get("expected_hash")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("create_dashboard requires expected_hash"))?;
        if request
            .parameters
            .as_object()
            .is_none_or(|value| value.len() != 1)
        {
            return Err(Error::invalid(
                "create_dashboard parameters may contain only expected_hash",
            ));
        }
        let draft = state
            .intelligence
            .dashboard_authoring
            .validate_reference(
                &ctx.org_id,
                &ctx.user_id,
                &Id(request.target.clone()),
                expected_hash,
            )
            .await?;
        Some(draft.expires_at)
    } else {
        None
    };
    let now = TimestampMicros::now();
    let required_approvals = if request.action == "create_dashboard" {
        request.required_approvals_override.unwrap_or(0).clamp(0, 2)
    } else {
        risk.required_approvals()
    };
    let item = ApprovalRequest {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        investigation_id: request.investigation_id,
        action: request.action,
        target: request.target,
        parameters: request.parameters,
        reason: request.reason,
        impact: request.impact,
        risk,
        status: if required_approvals == 0 {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Pending
        },
        requested_by: ctx.user_id.clone(),
        required_approvals,
        reviews: json!([]),
        expires_at: Some(TimestampMicros(dashboard_draft_expiry.map_or_else(
            || {
                request
                    .expires_at_micros
                    .unwrap_or(now.0 + DEFAULT_APPROVAL_TTL_MICROS)
            },
            |draft_expiry| {
                request
                    .expires_at_micros
                    .unwrap_or(now.0 + DEFAULT_APPROVAL_TTL_MICROS)
                    .min(draft_expiry.0)
            },
        ))),
        decided_at: if required_approvals == 0 {
            Some(now)
        } else {
            None
        },
        created_at: now,
        updated_at: now,
    };
    state.intelligence.repository.create_approval(item).await
}

#[permission("intelligence.use")]
async fn list_approvals(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    Ok(Json(json!({
        "approvals": state.intelligence.repository.list_approvals(&ctx.org_id).await?
    })))
}

#[permission("intelligence.use")]
async fn create_approval(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<CreateApprovalRequest>,
) -> Result<Json<ApprovalRequest>> {
    require_license(&state)?;
    let saved = create_agent_approval(&state, &ctx, request).await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.approval.requested",
        "intelligence_approval",
        &saved.id.0,
        json!({"action": saved.action, "target": saved.target, "risk": saved.risk}),
    )
    .await;
    Ok(Json(saved))
}

#[permission("intelligence.use")]
async fn get_approval(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ApprovalRequest>> {
    require_license(&state)?;
    Ok(Json(
        state
            .intelligence
            .repository
            .get_approval(&ctx.org_id, &Id(id))
            .await?,
    ))
}

#[derive(Debug, Deserialize)]
struct ReviewApprovalRequest {
    approve: bool,
    #[serde(default)]
    comment: String,
}

#[permission("intelligence.approve")]
async fn review_approval(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<ReviewApprovalRequest>,
) -> Result<Json<ApprovalRequest>> {
    require_license(&state)?;
    let id = Id(id);
    let existing = state
        .intelligence
        .repository
        .get_approval(&ctx.org_id, &id)
        .await?;
    if existing.required_approvals > 0 && existing.requested_by == ctx.user_id {
        return Err(Error::forbidden(
            "requester cannot approve their own reviewed operation",
        ));
    }
    let saved = state
        .intelligence
        .repository
        .review_approval(
            &ctx.org_id,
            &id,
            &ctx.user_id,
            request.approve,
            &request.comment,
            TimestampMicros::now(),
        )
        .await?;
    activity_audit::record(
        &state,
        &ctx,
        if request.approve {
            "intelligence.approval.approved"
        } else {
            "intelligence.approval.rejected"
        },
        "intelligence_approval",
        &saved.id.0,
        json!({"comment": request.comment, "status": saved.status}),
    )
    .await;
    Ok(Json(saved))
}

#[derive(Debug, Deserialize)]
struct ExecuteApprovalRequest {
    idempotency_key: String,
}

#[permission("intelligence.use")]
async fn execute_approval(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<ExecuteApprovalRequest>,
) -> Result<Json<Execution>> {
    require_license(&state)?;
    if request.idempotency_key.trim().is_empty() || request.idempotency_key.len() > 128 {
        return Err(Error::invalid(
            "idempotency_key length must be between 1 and 128",
        ));
    }
    if let Some(existing) = state
        .intelligence
        .repository
        .find_execution_by_key(&ctx.org_id, &request.idempotency_key)
        .await?
    {
        return Ok(Json(existing));
    }
    let approval_id = Id(id);
    let approval = state
        .intelligence
        .repository
        .get_approval(&ctx.org_id, &approval_id)
        .await?;
    if approval.status == ApprovalStatus::Executed {
        return state
            .intelligence
            .repository
            .list_executions(&ctx.org_id)
            .await?
            .into_iter()
            .find(|execution| execution.approval_request_id == approval.id)
            .map(Json)
            .ok_or_else(|| Error::internal("executed approval is missing its execution"));
    }
    if approval.status != ApprovalStatus::Approved {
        return Err(Error::conflict("approval has not reached approved status"));
    }
    if approval.required_approvals > 0 {
        Permission::require_key(&ctx, "intelligence.approve")?;
    } else if approval.requested_by != ctx.user_id {
        return Err(Error::forbidden(
            "only the requester can execute a confirmation-mode operation",
        ));
    }
    if approval
        .expires_at
        .is_some_and(|expires| expires.0 <= TimestampMicros::now().0)
    {
        return Err(Error::conflict("approval has expired"));
    }
    let (_, required_permission) = operation_policy(&approval.action)?;
    Permission::require_key(&ctx, required_permission)?;
    let approved_by = approval
        .reviews
        .as_array()
        .into_iter()
        .flatten()
        .filter(|review| review["decision"] == "approved")
        .filter_map(|review| review["reviewer_id"].as_str())
        .map(|id| Id(id.to_string()))
        .collect::<Vec<_>>();
    let now = TimestampMicros::now();
    let mut execution = Execution {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        approval_request_id: approval.id.clone(),
        investigation_id: approval.investigation_id.clone(),
        action: approval.action.clone(),
        target: approval.target.clone(),
        parameters: approval.parameters.clone(),
        idempotency_key: request.idempotency_key,
        requested_by: approval.requested_by.clone(),
        approved_by,
        status: ExecutionStatus::Running,
        output_summary: None,
        error: None,
        verification: json!({}),
        started_at: Some(now),
        finished_at: None,
        created_at: now,
        updated_at: now,
    };
    let proposed_execution_id = execution.id.clone();
    execution = state
        .intelligence
        .repository
        .create_execution(execution)
        .await?;
    if execution.id != proposed_execution_id {
        return Ok(Json(execution));
    }
    let outcome = run_registered_operation(&state, &ctx, &approval).await;
    let finished = TimestampMicros::now();
    match outcome {
        Ok(outcome) => {
            execution.status = ExecutionStatus::Succeeded;
            execution.output_summary = Some(outcome.summary);
            execution.verification = outcome.verification;
        }
        Err(error) => {
            execution.status = ExecutionStatus::Failed;
            execution.error = Some(error.to_string());
            execution.verification = json!({"verified": false});
        }
    }
    execution.finished_at = Some(finished);
    execution.updated_at = finished;
    execution = state
        .intelligence
        .repository
        .update_execution(execution)
        .await?;
    let _ = state
        .intelligence
        .repository
        .mark_approval_executed(&ctx.org_id, &approval_id, finished)
        .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.execution.completed",
        "intelligence_execution",
        &execution.id.0,
        json!({
            "approval_id": approval.id,
            "action": execution.action,
            "target": execution.target,
            "status": execution.status,
            "idempotency_key": execution.idempotency_key,
        }),
    )
    .await;
    Ok(Json(execution))
}

async fn run_registered_operation(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    if approval.action == "create_dashboard" {
        return dashboard_operation::execute(state, ctx, approval).await;
    }
    let incident_id = Id(approval.target.clone());
    let incident = state.alerting.service.get_incident(&incident_id).await?;
    if incident.org_id != ctx.org_id {
        return Err(Error::forbidden("alert belongs to another organization"));
    }
    match approval.action.as_str() {
        "acknowledge_alert" => match incident.status {
            IncidentStatus::Acknowledged => Ok(OperationOutcome::verified(
                "alert was already acknowledged; no change",
            )),
            IncidentStatus::Open => {
                state
                    .alerting
                    .service
                    .acknowledge(&incident_id, ctx.user_id.clone(), TimestampMicros::now())
                    .await?;
                Ok(OperationOutcome::verified(
                    "alert acknowledged and state re-read successfully",
                ))
            }
            _ => Err(Error::conflict("only an open alert can be acknowledged")),
        },
        "resolve_alert" => match incident.status {
            IncidentStatus::Resolved | IncidentStatus::Closed => Ok(OperationOutcome::verified(
                "alert was already resolved; no change",
            )),
            IncidentStatus::Open | IncidentStatus::Acknowledged => {
                state
                    .alerting
                    .service
                    .resolve(&incident_id, ctx.user_id.clone(), TimestampMicros::now())
                    .await?;
                Ok(OperationOutcome::verified(
                    "alert resolved and state re-read successfully",
                ))
            }
        },
        other => Err(Error::invalid(format!(
            "operation `{other}` is not registered"
        ))),
    }
}

pub(super) struct OperationOutcome {
    summary: String,
    verification: Value,
}

impl OperationOutcome {
    fn verified(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            verification: json!({"verified": true}),
        }
    }
}

#[permission("intelligence.use")]
async fn list_executions(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    Ok(Json(json!({
        "executions": state.intelligence.repository.list_executions(&ctx.org_id).await?
    })))
}

#[permission("intelligence.use")]
async fn get_execution(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Execution>> {
    require_license(&state)?;
    Ok(Json(
        state
            .intelligence
            .repository
            .get_execution(&ctx.org_id, &Id(id))
            .await?,
    ))
}

#[derive(Debug, Deserialize)]
struct AgentProfileRequest {
    name: String,
    #[serde(default)]
    description: String,
    model_provider_id: Option<Id>,
    model: Option<String>,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default)]
    data_scope: Value,
    #[serde(default)]
    risk_policy: Value,
    #[serde(default = "blocked_network")]
    network_access: NetworkAccess,
    #[serde(default = "default_context_tokens")]
    max_context_tokens: i32,
    #[serde(default = "default_investigation_secs")]
    max_investigation_secs: i32,
    #[serde(default = "default_tool_calls")]
    max_tool_calls: i32,
    #[serde(default)]
    is_default: bool,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn blocked_network() -> NetworkAccess {
    NetworkAccess::Blocked
}
fn default_context_tokens() -> i32 {
    32_000
}
fn default_investigation_secs() -> i32 {
    1_800
}
fn default_tool_calls() -> i32 {
    32
}

fn validate_profile_request(request: &AgentProfileRequest) -> Result<()> {
    if request.max_tool_calls < 1 || request.max_tool_calls > 256 {
        return Err(Error::invalid("max_tool_calls must be between 1 and 256"));
    }
    Ok(())
}

#[permission("intelligence.use")]
async fn list_profiles(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    Ok(Json(json!({
        "profiles": state.intelligence.repository.list_profiles(&ctx.org_id).await?
    })))
}

#[permission("intelligence.manage")]
async fn create_profile(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<AgentProfileRequest>,
) -> Result<Json<AgentProfile>> {
    require_license(&state)?;
    validate_profile_request(&request)?;
    validate_allowed_tools(&state, &ctx.org_id, &request.allowed_tools).await?;
    let now = TimestampMicros::now();
    let profile = AgentProfile {
        id: Id::new(),
        org_id: ctx.org_id,
        name: request.name,
        description: request.description,
        model_provider_id: request.model_provider_id,
        model: request.model,
        allowed_tools: request.allowed_tools,
        data_scope: request.data_scope,
        risk_policy: request.risk_policy,
        network_access: request.network_access,
        max_context_tokens: request.max_context_tokens,
        max_investigation_secs: request.max_investigation_secs,
        max_tool_calls: request.max_tool_calls,
        is_default: request.is_default,
        enabled: request.enabled,
        created_by: ctx.user_id,
        created_at: now,
        updated_at: now,
    };
    Ok(Json(
        state
            .intelligence
            .repository
            .create_profile(profile)
            .await?,
    ))
}

#[permission("intelligence.use")]
async fn get_profile(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<AgentProfile>> {
    require_license(&state)?;
    Ok(Json(
        state
            .intelligence
            .repository
            .get_profile(&ctx.org_id, &Id(id))
            .await?,
    ))
}

#[permission("intelligence.manage")]
async fn update_profile(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<AgentProfileRequest>,
) -> Result<Json<AgentProfile>> {
    require_license(&state)?;
    validate_profile_request(&request)?;
    validate_allowed_tools(&state, &ctx.org_id, &request.allowed_tools).await?;
    let existing = state
        .intelligence
        .repository
        .get_profile(&ctx.org_id, &Id(id))
        .await?;
    let profile = AgentProfile {
        name: request.name,
        description: request.description,
        model_provider_id: request.model_provider_id,
        model: request.model,
        allowed_tools: request.allowed_tools,
        data_scope: request.data_scope,
        risk_policy: request.risk_policy,
        network_access: request.network_access,
        max_context_tokens: request.max_context_tokens,
        max_investigation_secs: request.max_investigation_secs,
        max_tool_calls: request.max_tool_calls,
        is_default: request.is_default,
        enabled: request.enabled,
        updated_at: TimestampMicros::now(),
        ..existing
    };
    Ok(Json(
        state
            .intelligence
            .repository
            .update_profile(profile)
            .await?,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        AgentProfileRequest, UpdateInvestigationRequest, dashboard_required_approvals,
        operation_policy, validate_profile_request,
    };
    use crate::intelligence::{
        model::{NetworkAccess, RiskLevel},
        tool_control::ToolExecutionMode,
    };

    #[test]
    fn dashboard_policy_keeps_confirmation_floor_and_allows_only_tightening() {
        assert_eq!(
            operation_policy("create_dashboard").unwrap(),
            (RiskLevel::L1, "dashboards.create")
        );
        assert_eq!(
            dashboard_required_approvals(ToolExecutionMode::Automatic).unwrap(),
            0
        );
        assert_eq!(
            dashboard_required_approvals(ToolExecutionMode::Confirmation).unwrap(),
            0
        );
        assert_eq!(
            dashboard_required_approvals(ToolExecutionMode::SingleApproval).unwrap(),
            1
        );
        assert_eq!(
            dashboard_required_approvals(ToolExecutionMode::DualApproval).unwrap(),
            2
        );
        assert!(dashboard_required_approvals(ToolExecutionMode::Disabled).is_err());
    }

    #[test]
    fn investigation_update_distinguishes_missing_null_and_value() {
        let missing: UpdateInvestigationRequest = serde_json::from_value(json!({})).unwrap();
        assert!(missing.summary.is_none());

        let cleared: UpdateInvestigationRequest =
            serde_json::from_value(json!({"summary": null, "current_step": null})).unwrap();
        assert_eq!(cleared.summary, Some(None));
        assert_eq!(cleared.current_step, Some(None));

        let populated: UpdateInvestigationRequest =
            serde_json::from_value(json!({"summary": "Recovered", "confidence": "high"})).unwrap();
        assert_eq!(populated.summary, Some(Some("Recovered".to_owned())));
        assert!(matches!(populated.confidence, Some(Some(_))));
    }

    #[test]
    fn profile_network_access_accepts_blocked_and_allowed() {
        let defaulted: AgentProfileRequest =
            serde_json::from_value(json!({"name": "default"})).unwrap();
        assert_eq!(defaulted.network_access, NetworkAccess::Blocked);
        assert!(validate_profile_request(&defaulted).is_ok());

        let allowed: AgentProfileRequest =
            serde_json::from_value(json!({"name": "networked", "network_access": "allowed"}))
                .unwrap();
        assert_eq!(allowed.network_access, NetworkAccess::Allowed);
        assert!(validate_profile_request(&allowed).is_ok());

        assert!(
            serde_json::from_value::<AgentProfileRequest>(
                json!({"name": "invalid", "network_access": "unrestricted"}),
            )
            .is_err()
        );
    }
}
