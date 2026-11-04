// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What people keep about services: a service's definition (who owns it, how
//! much it matters, where it is in its life) and what is being done about
//! each error group. Owners and assignees are the platform's teams and users.

use std::collections::{BTreeMap, HashSet};

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{apm::ErrorIssueView, iam::IamContext},
    domain::{
        apm::{
            ErrorIssue, ErrorIssueStatus, KeyTransactionMark, ServiceDefinition, ServiceLifecycle,
            ServiceTier, TransactionIdentity, masked_repository_address,
        },
        iam::permission,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/apm/service-definitions", get(list_definitions))
        .route(
            "/apm/service-definitions/{service}",
            put(save_definition).delete(delete_definition),
        )
        .route(
            "/apm/service-definitions/{service}/key-transactions",
            post(mark_key_transaction),
        )
        .route("/apm/errors/{fingerprint}/issue", put(save_issue))
}

#[derive(Debug, Deserialize)]
struct NamespaceParams {
    namespace: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefinitionWrite {
    #[serde(default)]
    description: String,
    team_id: Option<String>,
    owner_id: Option<String>,
    tier: Option<ServiceTier>,
    lifecycle: Option<ServiceLifecycle>,
    #[serde(default)]
    repository_url: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
    #[serde(default)]
    apdex_threshold: Option<u64>,
    /// Left out, a definition already kept stays as archived as it was.
    #[serde(default)]
    archived: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct KeyTransactionWrite {
    transaction: TransactionIdentity,
    /// Starred, or the star taken off.
    key: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueWrite {
    status: ErrorIssueStatus,
    assignee_id: Option<String>,
    team_id: Option<String>,
    /// The last time the group was seen, as the reader saw it: a regression
    /// is judged against when it was resolved.
    last_seen_at: Option<i64>,
}

#[permission(any("apm.definitions.read", "sys.telemetry.read"))]
async fn list_definitions(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ServiceDefinition>>> {
    Ok(Json(
        state
            .telemetry
            .apm_catalog
            .list_definitions(&ctx.org_id)
            .await?,
    ))
}

#[permission("apm.definitions.manage")]
async fn save_definition(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(service): Path<String>,
    Query(params): Query<NamespaceParams>,
    Json(request): Json<DefinitionWrite>,
) -> Result<Json<ServiceDefinition>> {
    let team_id = present(request.team_id);
    let owner_id = present(request.owner_id);

    if let Some(team_id) = &team_id {
        ensure_team(&state, &ctx.org_id, team_id).await?;
    }

    if let Some(owner_id) = &owner_id {
        ensure_members(&state, &ctx.org_id, [owner_id]).await?;
    }

    let namespace = params.namespace.trim().to_owned();
    let name = service.trim().to_owned();
    let kept = state
        .telemetry
        .apm_catalog
        .list_definitions(&ctx.org_id)
        .await?
        .into_iter()
        .find(|definition| definition.namespace == namespace && definition.name == name);

    let definition = ServiceDefinition {
        description: request.description.trim().to_owned(),
        team_id,
        owner_id,
        tier: request.tier,
        lifecycle: request.lifecycle,
        // Masked before anything keeps it; what is no address at all is left for the check to refuse.
        repository_url: request
            .repository_url
            .map(|address| address.trim().to_owned())
            .filter(|address| !address.is_empty())
            .map(|address| masked_repository_address(&address).unwrap_or(address)),
        tags: trimmed_tags(request.tags),
        apdex_threshold: request.apdex_threshold,
        // Starred one at a time, and kept as they are by a save.
        key_transactions: kept
            .as_ref()
            .map(|kept| kept.key_transactions.clone())
            .unwrap_or_default(),
        archived: request
            .archived
            .unwrap_or_else(|| kept.as_ref().is_some_and(|kept| kept.archived)),
        ..ServiceDefinition::blank(
            &namespace,
            &name,
            ctx.principal_id().clone(),
            TimestampMicros::now(),
        )
    };

    definition.validate()?;
    state
        .telemetry
        .apm_catalog
        .save_definition(&ctx.org_id, &definition)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "apm.service_definition.save",
        "apm_service",
        &format!("{}/{}", definition.namespace, definition.name),
        serde_json::json!({
            "teamId": definition.team_id,
            "ownerId": definition.owner_id,
            "tier": definition.tier,
            "lifecycle": definition.lifecycle,
            "tags": definition.tags,
            "apdexThreshold": definition.apdex_threshold,
            "archived": definition.archived,
        }),
    )
    .await;

    Ok(Json(definition))
}

/// Stars an endpoint of a service as a key transaction, or takes the star
/// off, without rewriting the rest of what is kept about the service.
#[permission("apm.definitions.manage")]
async fn mark_key_transaction(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(service): Path<String>,
    Query(params): Query<NamespaceParams>,
    Json(request): Json<KeyTransactionWrite>,
) -> Result<Json<ServiceDefinition>> {
    let mark = KeyTransactionMark {
        namespace: params.namespace.trim().to_owned(),
        name: service.trim().to_owned(),
        transaction: request.transaction,
        key: request.key,
        updated_by: ctx.principal_id().clone(),
        updated_at: TimestampMicros::now(),
    };

    ServiceDefinition::blank(
        &mark.namespace,
        &mark.name,
        mark.updated_by.clone(),
        mark.updated_at,
    )
    .validate()?;

    let definition = state
        .telemetry
        .apm_catalog
        .mark_key_transaction(&ctx.org_id, &mark)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "apm.key_transaction.mark",
        "apm_service",
        &format!("{}/{}", mark.namespace, mark.name),
        serde_json::json!({"transaction": mark.transaction, "key": mark.key}),
    )
    .await;

    Ok(Json(definition))
}

/// Tags as they are kept: keys and values without the spaces around them.
fn trimmed_tags(tags: BTreeMap<String, String>) -> BTreeMap<String, String> {
    tags.into_iter()
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect()
}

#[permission("apm.definitions.manage")]
async fn delete_definition(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(service): Path<String>,
    Query(params): Query<NamespaceParams>,
) -> Result<StatusCode> {
    let deleted = state
        .telemetry
        .apm_catalog
        .delete_definition(&ctx.org_id, params.namespace.trim(), service.trim())
        .await?;

    if !deleted {
        return Err(Error::not_found("APM service definition"));
    }

    activity_audit::record(
        &state,
        &ctx,
        "apm.service_definition.delete",
        "apm_service",
        &format!("{}/{}", params.namespace.trim(), service.trim()),
        serde_json::json!({}),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

#[permission("apm.errors.manage")]
async fn save_issue(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(fingerprint): Path<String>,
    Json(request): Json<IssueWrite>,
) -> Result<Json<ErrorIssueView>> {
    if fingerprint.is_empty() || fingerprint.len() > 128 {
        return Err(Error::not_found("APM error group"));
    }

    let assignee_id = present(request.assignee_id);
    let team_id = present(request.team_id);

    if let Some(assignee_id) = &assignee_id {
        ensure_members(&state, &ctx.org_id, [assignee_id]).await?;
    }

    if let Some(team_id) = &team_id {
        ensure_team(&state, &ctx.org_id, team_id).await?;
    }

    let now = TimestampMicros::now();
    let issue = ErrorIssue {
        fingerprint,
        status: request.status,
        assignee_id,
        team_id,
        resolved_at: (request.status == ErrorIssueStatus::Resolved).then_some(now),
        updated_by: ctx.principal_id().clone(),
        updated_at: now,
    };

    if !state
        .telemetry
        .apm_catalog
        .save_issue(&ctx.org_id, &issue)
        .await?
    {
        return Err(Error::not_found("APM error group"));
    }

    activity_audit::record(
        &state,
        &ctx,
        "apm.error_issue.save",
        "apm_error_group",
        &issue.fingerprint,
        serde_json::json!({
            "status": issue.status,
            "assigneeId": issue.assignee_id,
            "teamId": issue.team_id,
        }),
    )
    .await;

    let last_seen_at = TimestampMicros(request.last_seen_at.unwrap_or(now.0));

    Ok(Json(ErrorIssueView::of(issue, last_seen_at)))
}

/// An id that was sent, as opposed to an empty one.
fn present(value: Option<String>) -> Option<Id> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(Id::from_string)
}

/// Every user given is a member of the organization.
async fn ensure_team(state: &AppState, org_id: &Id, team_id: &Id) -> Result<()> {
    let team = state.iam.teams.get(team_id).await?;

    if &team.org_id != org_id {
        return Err(Error::invalid(
            "teamId must reference a team in this organization",
        ));
    }

    Ok(())
}

async fn ensure_members<'a>(
    state: &AppState,
    org_id: &Id,
    user_ids: impl IntoIterator<Item = &'a Id>,
) -> Result<()> {
    let members = state
        .iam
        .service
        .iam_memberships
        .list_for_org(org_id)
        .await?
        .into_iter()
        .map(|membership| membership.user_id.0)
        .collect::<HashSet<_>>();

    if user_ids
        .into_iter()
        .any(|id| !members.contains(id.as_str()))
    {
        return Err(Error::invalid(
            "the user must be a member of this organization",
        ));
    }

    Ok(())
}
