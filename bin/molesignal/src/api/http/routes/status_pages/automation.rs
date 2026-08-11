// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{
        iam::IamContext,
        status_page::{
            AutomationApprovalInput, AutomationLifecycleInput, AutomationRuleInput,
            AutomationRuleOrderInput, AutomationSettingsInput,
        },
    },
    domain::{
        alerting::incident::Severity,
        iam::permission,
        status_page::{
            ActiveAutomationRule, AutomationCandidate, AutomationCandidateDetail,
            AutomationCandidateState, AutomationSourceKind, AutomationSourceObservation,
            StatusAutomationRevision, StatusAutomationRule,
        },
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/status-pages/automation/candidates/pending",
            get(list_pending_candidates),
        )
        .route(
            "/status-pages/{page_id}/automation/rules",
            get(list_rules).post(create_rule),
        )
        .route(
            "/status-pages/{page_id}/automation/rules/order",
            axum::routing::put(reorder_rules),
        )
        .route(
            "/status-pages/{page_id}/automation/rules/{rule_id}/revisions",
            post(create_revision),
        )
        .route(
            "/status-pages/{page_id}/automation/rules/{rule_id}/revisions/{revision_id}/activate",
            post(activate_revision),
        )
        .route(
            "/status-pages/{page_id}/automation/rules/{rule_id}/lifecycle",
            axum::routing::put(set_lifecycle),
        )
        .route(
            "/status-pages/{page_id}/automation/simulate",
            post(simulate),
        )
        .route(
            "/status-pages/{page_id}/automation/candidates",
            get(list_candidates),
        )
        .route(
            "/status-pages/{page_id}/automation/candidates/{candidate_id}",
            get(get_candidate),
        )
        .route(
            "/status-pages/{page_id}/automation/candidates/{candidate_id}/approve",
            post(approve_candidate),
        )
        .route(
            "/status-pages/{page_id}/automation/candidates/{candidate_id}/reject",
            post(reject_candidate),
        )
        .route(
            "/status-pages/{page_id}/automation/candidates/{candidate_id}/retry",
            post(retry_candidate),
        )
        .route(
            "/status-pages/{page_id}/automation/settings",
            get(get_settings).put(set_settings),
        )
}

#[derive(Debug, Deserialize)]
struct SimulationRequest {
    source_kind: AutomationSourceKind,
    source_id: Id,
    #[serde(default)]
    source_instance_id: Option<Id>,
    severity: Severity,
    #[serde(default)]
    labels: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct CandidateQuery {
    state: Option<AutomationCandidateState>,
    #[serde(default = "default_limit")]
    limit: u32,
}

#[derive(Debug, Deserialize)]
struct DecisionRequest {
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Serialize)]
struct SettingsView {
    paused: bool,
}

const fn default_limit() -> u32 {
    100
}

#[permission("status_pages.read")]
async fn list_rules(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<Vec<ActiveAutomationRule>>> {
    Ok(Json(
        state
            .status_pages
            .list_automation_rules(&context.org_id, &Id(page_id))
            .await?,
    ))
}

#[permission("status_pages.manage")]
async fn create_rule(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Json(input): Json<AutomationRuleInput>,
) -> Result<(StatusCode, Json<ActiveAutomationRule>)> {
    let page_id = Id(page_id);
    let rule = state
        .status_pages
        .create_automation_rule(&context.org_id, &page_id, &context.user_id, input)
        .await?;
    audit_rule(&state, &context, &rule.rule, "create").await;
    Ok((StatusCode::CREATED, Json(rule)))
}

#[permission("status_pages.manage")]
async fn reorder_rules(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Json(input): Json<AutomationRuleOrderInput>,
) -> Result<Json<Vec<ActiveAutomationRule>>> {
    let page_id = Id(page_id);
    let rules = state
        .status_pages
        .reorder_automation_rules(&context.org_id, &page_id, &input.rule_ids)
        .await?;
    activity_audit::record(
        &state,
        &context,
        "status_page.automation_rule.reorder",
        "status_page",
        page_id.as_str(),
        serde_json::json!({ "rule_ids": input.rule_ids }),
    )
    .await;
    Ok(Json(rules))
}

#[permission("status_pages.manage")]
async fn create_revision(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, rule_id)): Path<(String, String)>,
    Json(input): Json<AutomationRuleInput>,
) -> Result<(StatusCode, Json<StatusAutomationRevision>)> {
    let revision = state
        .status_pages
        .create_automation_revision(
            &context.org_id,
            &Id(page_id),
            &Id(rule_id),
            &context.user_id,
            input,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(revision)))
}

#[permission("status_pages.manage")]
async fn activate_revision(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, rule_id, revision_id)): Path<(String, String, String)>,
) -> Result<Json<ActiveAutomationRule>> {
    let active = state
        .status_pages
        .activate_automation_revision(
            &context.org_id,
            &Id(page_id),
            &Id(rule_id),
            &Id(revision_id),
        )
        .await?;
    audit_rule(&state, &context, &active.rule, "activate").await;
    Ok(Json(active))
}

#[permission("status_pages.manage")]
async fn set_lifecycle(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, rule_id)): Path<(String, String)>,
    Json(input): Json<AutomationLifecycleInput>,
) -> Result<Json<StatusAutomationRule>> {
    let rule = state
        .status_pages
        .set_automation_rule_lifecycle(&context.org_id, &Id(page_id), &Id(rule_id), input.lifecycle)
        .await?;
    audit_rule(&state, &context, &rule, rule.lifecycle.as_str()).await;
    Ok(Json(rule))
}

#[permission("status_pages.manage")]
async fn simulate(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Json(input): Json<SimulationRequest>,
) -> Result<Json<crate::app::status_page::AutomationSimulation>> {
    let source_instance_id = input
        .source_instance_id
        .unwrap_or_else(|| input.source_id.clone());
    Ok(Json(
        state
            .status_pages
            .simulate_automation(
                &AutomationSourceObservation {
                    organization_id: context.org_id,
                    source_kind: input.source_kind,
                    source_id: input.source_id,
                    source_instance_id,
                    severity: input.severity,
                    labels: input.labels,
                    active: true,
                    muted: false,
                    observed_at: TimestampMicros::now(),
                },
                &Id(page_id),
            )
            .await?,
    ))
}

#[permission("status_pages.read")]
async fn list_candidates(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Query(query): Query<CandidateQuery>,
) -> Result<Json<Vec<AutomationCandidate>>> {
    Ok(Json(
        state
            .status_pages
            .list_automation_candidates(&context.org_id, &Id(page_id), query.state, query.limit)
            .await?,
    ))
}

#[permission("status_pages.publish")]
async fn list_pending_candidates(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<AutomationCandidate>>> {
    Ok(Json(
        state
            .status_pages
            .list_pending_automation_candidates(&context.org_id, 100)
            .await?,
    ))
}

#[permission("status_pages.publish")]
async fn get_candidate(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, candidate_id)): Path<(String, String)>,
) -> Result<Json<AutomationCandidateDetail>> {
    Ok(Json(
        state
            .status_pages
            .get_automation_candidate_detail(&context.org_id, &Id(page_id), &Id(candidate_id))
            .await?,
    ))
}

#[permission("status_pages.publish")]
async fn approve_candidate(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, candidate_id)): Path<(String, String)>,
    Json(input): Json<AutomationApprovalInput>,
) -> Result<Json<AutomationCandidate>> {
    let candidate = state
        .status_pages
        .approve_automation_candidate(
            &context.org_id,
            &Id(page_id),
            &Id(candidate_id),
            &context.user_id,
            input,
        )
        .await?;
    audit_candidate(&state, &context, &candidate, "approve").await;
    Ok(Json(candidate))
}

#[permission("status_pages.publish")]
async fn reject_candidate(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, candidate_id)): Path<(String, String)>,
    Json(input): Json<DecisionRequest>,
) -> Result<Json<AutomationCandidate>> {
    let candidate = state
        .status_pages
        .reject_automation_candidate(
            &context.org_id,
            &Id(page_id),
            &Id(candidate_id),
            &context.user_id,
            input.note.as_deref(),
        )
        .await?;
    audit_candidate(&state, &context, &candidate, "reject").await;
    Ok(Json(candidate))
}

#[permission("status_pages.publish")]
async fn retry_candidate(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, candidate_id)): Path<(String, String)>,
) -> Result<Json<AutomationCandidate>> {
    let candidate = state
        .status_pages
        .retry_automation_candidate(
            &context.org_id,
            &Id(page_id),
            &Id(candidate_id),
            &context.user_id,
        )
        .await?;
    audit_candidate(&state, &context, &candidate, "retry").await;
    Ok(Json(candidate))
}

#[permission("status_pages.read")]
async fn get_settings(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<SettingsView>> {
    let settings = state
        .status_pages
        .get_automation_settings(&context.org_id, &Id(page_id))
        .await?;
    Ok(Json(SettingsView {
        paused: settings.is_some_and(|settings| settings.paused),
    }))
}

#[permission("status_pages.manage")]
async fn set_settings(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Json(input): Json<AutomationSettingsInput>,
) -> Result<Json<SettingsView>> {
    let page_id = Id(page_id);
    let settings = state
        .status_pages
        .set_automation_paused(&context.org_id, &page_id, &context.user_id, input.paused)
        .await?;
    activity_audit::record(
        &state,
        &context,
        if settings.paused {
            "status_page.automation.pause"
        } else {
            "status_page.automation.resume"
        },
        "status_page",
        page_id.as_str(),
        serde_json::json!({ "paused": settings.paused }),
    )
    .await;
    Ok(Json(SettingsView {
        paused: settings.paused,
    }))
}

async fn audit_rule(
    state: &AppState,
    context: &IamContext,
    rule: &StatusAutomationRule,
    verb: &str,
) {
    activity_audit::record(
        state,
        context,
        &format!("status_page.automation_rule.{verb}"),
        "status_page_automation_rule",
        rule.id.as_str(),
        serde_json::json!({
            "status_page_id": rule.status_page_id,
            "lifecycle": rule.lifecycle,
        }),
    )
    .await;
}

async fn audit_candidate(
    state: &AppState,
    context: &IamContext,
    candidate: &AutomationCandidate,
    verb: &str,
) {
    activity_audit::record(
        state,
        context,
        &format!("status_page.automation_candidate.{verb}"),
        "status_page_automation_candidate",
        candidate.id.as_str(),
        serde_json::json!({
            "status_page_id": candidate.status_page_id,
            "state": candidate.state,
            "automatic": candidate.automatic,
        }),
    )
    .await;
}
