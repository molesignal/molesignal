// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 调查、审批、执行与 Agent Profile API。自动化在 `automations`。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use tool_runtime::{ToolSurface, catalog::tools_for_surface};

use crate::{
    agent::{
        model::{
            ApprovalRequest, ApprovalStatus, ConfidenceLevel, Execution, FactStatus,
            HypothesisStatus, Investigation, InvestigationEvidence, InvestigationHypothesis,
            InvestigationStatus, InvestigationStep, StepStatus,
        },
        tools::builtin_tools,
    },
    api::{
        AppState,
        http::{middleware::Permission, routes::activity_audit},
    },
    app::{
        iam::IamContext,
        tools::dashboard::{CreateApprovalRequest, create_agent_approval},
    },
    domain::iam::permission,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod approval_execution;
mod dashboard_operation;
mod execution_response;
mod incident_operation;
mod profiles;
pub(crate) mod remediation_operation;
mod resource_operation;

use approval_execution::OperationOutcome;
pub(super) use approval_execution::{OperationExecution, execute_approved_operation};
use profiles::{create_profile, get_profile, list_profiles, update_profile};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/overview", get(overview))
        .route(
            "/agent/investigations",
            get(list_investigations).post(create_investigation),
        )
        .route(
            "/agent/investigations/{id}",
            get(get_investigation).put(update_investigation),
        )
        .route(
            "/agent/investigations/{id}/steps",
            post(append_investigation_step),
        )
        .route(
            "/agent/investigations/{id}/evidence",
            post(append_investigation_evidence),
        )
        .route(
            "/agent/investigations/{id}/hypotheses",
            post(upsert_investigation_hypothesis),
        )
        .route(
            "/agent/approvals",
            get(list_approvals).post(create_approval),
        )
        .route("/agent/approvals/{id}", get(get_approval))
        .route("/agent/approvals/{id}/review", post(review_approval))
        .route("/agent/approvals/{id}/execute", post(execute_approval))
        .route("/agent/executions", get(list_executions))
        .route("/agent/executions/{id}", get(get_execution))
        .route(
            "/agent/settings/agent-profiles",
            get(list_profiles).post(create_profile),
        )
        .route(
            "/agent/settings/agent-profiles/{id}",
            get(get_profile).put(update_profile),
        )
}

#[permission("agent.use")]
async fn overview(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let investigations = state
        .agent
        .repository
        .list_investigations(&ctx.org_id)
        .await?;

    let approvals = state.agent.repository.list_approvals(&ctx.org_id).await?;
    let executions = state.agent.repository.list_executions(&ctx.org_id).await?;
    let automations = state.agent.automations.list(&ctx.org_id).await?;

    Ok(Json(json!({
        "activeInvestigations": investigations.iter().filter(|item| matches!(
            item.status,
            InvestigationStatus::Pending
                | InvestigationStatus::Running
                | InvestigationStatus::WaitingForData
                | InvestigationStatus::WaitingForApproval
                | InvestigationStatus::VerifyingRecovery
        )).count(),
        "pendingApprovals": approvals.iter().filter(|item| item.status == ApprovalStatus::Pending).count(),
        "recentCompleted": investigations.iter().filter(|item| item.status == InvestigationStatus::Completed).count(),
        "automationRuns": executions.iter().filter(|item| item.investigation_id.is_none()).count(),
        "enabledAutomations": automations.iter().filter(|item| item.enabled).count(),
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateInvestigationRequest {
    title: String,
    #[serde(default)]
    chat_id: Option<Id>,
    #[serde(default)]
    context: Value,
    #[serde(default)]
    steps: Vec<String>,
}

#[permission("agent.investigations.read")]
async fn list_investigations(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    Ok(Json(json!({
        "investigations": state.agent.repository.list_investigations(&ctx.org_id).await?
    })))
}

#[permission("agent.investigations.manage")]
async fn create_investigation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<CreateInvestigationRequest>,
) -> Result<Json<Investigation>> {
    if request.title.trim().is_empty() {
        return Err(Error::invalid("investigation title cannot be empty"));
    }

    let now = TimestampMicros::now();

    let item = Investigation {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        created_by: ctx.principal_id().clone(),
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

    let saved = state.agent.repository.create_investigation(item).await?;

    for (position, title) in request.steps.into_iter().enumerate() {
        if title.trim().is_empty() {
            continue;
        }

        state
            .agent
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
        "agent.investigation.created",
        "agent_investigation",
        &saved.id.0,
        json!({"title": saved.title}),
    )
    .await;

    Ok(Json(saved))
}

#[permission("agent.investigations.read")]
async fn get_investigation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(
        serde_json::to_value(
            state
                .agent
                .repository
                .get_investigation(&ctx.org_id, &Id(id))
                .await?,
        )
        .unwrap_or(Value::Null),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[permission("agent.investigations.manage")]
async fn update_investigation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<UpdateInvestigationRequest>,
) -> Result<Json<Investigation>> {
    let mut item = state
        .agent
        .repository
        .get_investigation(&ctx.org_id, &Id(id))
        .await?
        .investigation;

    if &item.created_by != ctx.principal_id() {
        Permission::require_key(&ctx, "agent.manage")?;
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
        state.agent.repository.update_investigation(item).await?,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    started_at: Option<i64>,
    ended_at: Option<i64>,
}

#[permission("agent.investigations.manage")]
async fn append_investigation_step(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<AppendStepRequest>,
) -> Result<Json<InvestigationStep>> {
    let investigation_id = Id(id);

    state
        .agent
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
        org_id: ctx.org_id.clone(),
        position: request.position,
        title: request.title,
        status: request.status.unwrap_or(StepStatus::Pending),
        tool_name: request.tool_name,
        input: request.input,
        output_summary: request.output_summary,
        conclusion_impact: request.conclusion_impact,
        error: request.error,
        started_at: request.started_at.map(TimestampMicros),
        ended_at: request.ended_at.map(TimestampMicros),
        created_at: TimestampMicros::now(),
    };

    Ok(Json(state.agent.repository.append_step(item).await?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[permission("agent.investigations.manage")]
async fn append_investigation_evidence(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<AppendEvidenceRequest>,
) -> Result<Json<InvestigationEvidence>> {
    let investigation_id = Id(id);

    state
        .agent
        .repository
        .get_investigation(&ctx.org_id, &investigation_id)
        .await?;

    let item = InvestigationEvidence {
        id: Id::new(),
        investigation_id,
        step_id: request.step_id,
        org_id: ctx.org_id.clone(),
        kind: request.kind,
        label: request.label,
        fact_status: request.fact_status,
        source_ref: request.source_ref,
        query: request.query,
        parameters: request.parameters,
        summary: request.summary,
        created_at: TimestampMicros::now(),
    };

    Ok(Json(state.agent.repository.append_evidence(item).await?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpsertHypothesisRequest {
    id: Option<Id>,
    statement: String,
    confidence: ConfidenceLevel,
    status: HypothesisStatus,
    #[serde(default)]
    evidence_ids: Vec<Id>,
}

#[permission("agent.investigations.manage")]
async fn upsert_investigation_hypothesis(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<UpsertHypothesisRequest>,
) -> Result<Json<InvestigationHypothesis>> {
    let investigation_id = Id(id);

    state
        .agent
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

    Ok(Json(state.agent.repository.upsert_hypothesis(item).await?))
}

fn default_true() -> bool {
    true
}

async fn validate_allowed_tools(
    state: &AppState,
    org_id: &Id,
    tools: &[String],
    surface: ToolSurface,
) -> Result<()> {
    let mut registered = tools_for_surface(surface)
        .into_iter()
        .map(|tool| tool.name)
        .collect::<std::collections::HashSet<_>>();

    registered.extend(
        state
            .agent
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

#[permission("agent.approvals.read")]
async fn list_approvals(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    Ok(Json(json!({
        "approvals": state.agent.repository.list_approvals(&ctx.org_id).await?
    })))
}

#[permission("agent.approvals.request")]
async fn create_approval(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<CreateApprovalRequest>,
) -> Result<Json<ApprovalRequest>> {
    let saved = create_agent_approval(&state.tools, &ctx, request).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.approval.requested",
        "agent_approval",
        &saved.id.0,
        json!({"action": saved.action, "target": saved.target, "risk": saved.risk}),
    )
    .await;

    Ok(Json(saved))
}

#[permission("agent.approvals.read")]
async fn get_approval(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ApprovalRequest>> {
    Ok(Json(
        state
            .agent
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

#[permission("agent.approve")]
async fn review_approval(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<ReviewApprovalRequest>,
) -> Result<axum::response::Response> {
    let id = Id(id);

    let existing = state
        .agent
        .repository
        .get_approval(&ctx.org_id, &id)
        .await?;

    if existing.required_approvals > 0 && &existing.requested_by == ctx.principal_id() {
        return Err(Error::forbidden(
            "requester cannot approve their own reviewed operation",
        ));
    }

    let mut saved = state
        .agent
        .repository
        .review_approval(
            &ctx.org_id,
            &id,
            ctx.principal_id(),
            request.approve,
            &request.comment,
            TimestampMicros::now(),
        )
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        if request.approve {
            "agent.approval.approved"
        } else {
            "agent.approval.rejected"
        },
        "agent_approval",
        &saved.id.0,
        json!({"comment": request.comment, "status": saved.status}),
    )
    .await;

    let execution = if request.approve && saved.status == ApprovalStatus::Approved {
        let execution = execute_approved_operation(
            &state,
            &ctx,
            &saved.id,
            format!("approval:{}:approved", saved.id.0),
        )
        .await?;

        saved = state
            .agent
            .repository
            .get_approval(&ctx.org_id, &saved.id)
            .await?;

        Some(execution)
    } else {
        None
    };

    Ok(execution_response::review_response(saved, execution))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteApprovalRequest {
    idempotency_key: String,
}

#[permission("agent.approvals.request")]
async fn execute_approval(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<ExecuteApprovalRequest>,
) -> Result<axum::response::Response> {
    let result = execute_approved_operation(&state, &ctx, &Id(id), request.idempotency_key).await?;
    Ok(execution_response::response(result))
}

#[permission("agent.investigations.read")]
async fn list_executions(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    Ok(Json(json!({
        "executions": state.agent.repository.list_executions(&ctx.org_id).await?
    })))
}

#[permission("agent.investigations.read")]
async fn get_execution(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Execution>> {
    Ok(Json(
        state
            .agent
            .repository
            .get_execution(&ctx.org_id, &Id(id))
            .await?,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::UpdateInvestigationRequest;
    use crate::{
        agent::{model::RiskLevel, tool_control::ToolExecutionMode},
        app::tools::dashboard::{dashboard_required_approvals, operation_policy},
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
            serde_json::from_value(json!({"summary": null, "currentStep": null})).unwrap();
        assert_eq!(cleared.summary, Some(None));
        assert_eq!(cleared.current_step, Some(None));

        let populated: UpdateInvestigationRequest =
            serde_json::from_value(json!({"summary": "Recovered", "confidence": "high"})).unwrap();
        assert_eq!(populated.summary, Some(Some("Recovered".to_owned())));
        assert!(matches!(populated.confidence, Some(Some(_))));
    }
}
