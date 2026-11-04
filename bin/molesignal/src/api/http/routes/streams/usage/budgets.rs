// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Budgets on sources. A source has at most one: setting it again changes that one. A budget only
//! tells people, it never touches intake.

use std::collections::HashSet;

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{BudgetView, refuse_unseen, standings_of};
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, intake::usage::SourceLabel},
    domain::{
        iam::permission,
        intake::usage::{
            budget::{BudgetPeriod, DEFAULT_BUDGET_WARN_PERCENT, IntakeBudget},
            source::UsageDimension,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const AUDIT_TARGET: &str = "intake_budget";

#[derive(Debug, Serialize)]
struct BudgetListItem {
    label: SourceLabel,
    #[serde(flatten)]
    budget: BudgetView,
}

#[derive(Debug, Serialize)]
pub(in super::super) struct BudgetListResponse {
    budgets: Vec<BudgetListItem>,
}

/// Every budget of the organization with where it stands.
#[permission(any("intake_usage.read", "sys.telemetry.read"))]
pub(in super::super) async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<BudgetListResponse>> {
    let budgets = state.platform.intake_budgets.list(&ctx.org_id).await?;
    let standings = standings_of(&state, &ctx, &budgets, TimestampMicros::now()).await;
    let mut items = Vec::with_capacity(budgets.len());

    for dimension in UsageDimension::ALL {
        let keys = budgets
            .iter()
            .filter(|budget| budget.dimension == dimension)
            .map(|budget| budget.source_key.as_str())
            .collect::<Vec<_>>();

        if keys.is_empty() {
            continue;
        }

        let book = state
            .platform
            .intake_sources
            .book(&ctx.org_id, dimension, &keys)
            .await;

        for (budget, standing) in budgets.iter().zip(&standings) {
            if budget.dimension != dimension {
                continue;
            }

            let Some(label) = book.labels.get(&budget.source_key) else {
                continue;
            };

            items.push(BudgetListItem {
                label: label.clone(),
                budget: BudgetView::new(budget, standing.clone()),
            });
        }
    }

    Ok(Json(BudgetListResponse { budgets: items }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct SetBudgetRequest {
    dimension: UsageDimension,
    source_key: String,
    period: BudgetPeriod,
    /// In MiB.
    limit_size: u64,
    warn_percent: Option<u8>,
    /// Who is told. Left out, the person setting the budget.
    notify_user_ids: Option<Vec<String>>,
}

#[permission("intake_budgets.manage")]
pub(in super::super) async fn set(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<SetBudgetRequest>,
) -> Result<Json<BudgetView>> {
    refuse_platform_organization(&ctx)?;
    refuse_unseen(&ctx, request.dimension, &request.source_key)?;

    let notify_user_ids = recipients(&state, &ctx, request.notify_user_ids).await?;
    let now = TimestampMicros::now();

    let budget = IntakeBudget {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        dimension: request.dimension,
        source_key: request.source_key,
        period: request.period,
        limit_size: request.limit_size,
        warn_percent: request.warn_percent.unwrap_or(DEFAULT_BUDGET_WARN_PERCENT),
        notify_user_ids,
        created_by: ctx.principal_id().clone(),
        updated_by: ctx.principal_id().clone(),
        created_at: now,
        updated_at: now,
    };

    budget.validate()?;
    refuse_missing_source(&state, &ctx, &budget).await?;

    let saved = state.platform.intake_budgets.upsert(budget).await?;

    activity_audit::record(
        &state,
        &ctx,
        "intake_budget.set",
        AUDIT_TARGET,
        saved.id.as_str(),
        audit_payload(&saved),
    )
    .await;

    let standing = standings_of(&state, &ctx, std::slice::from_ref(&saved), now)
        .await
        .remove(0);

    Ok(Json(BudgetView::new(&saved, standing)))
}

#[permission("intake_budgets.manage")]
pub(in super::super) async fn remove(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(budget_id): Path<String>,
) -> Result<axum::http::StatusCode> {
    let removed = state
        .platform
        .intake_budgets
        .delete(&ctx.org_id, &Id::from_string(budget_id))
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "intake_budget.remove",
        AUDIT_TARGET,
        removed.id.as_str(),
        audit_payload(&removed),
    )
    .await;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

fn audit_payload(budget: &IntakeBudget) -> serde_json::Value {
    json!({
        "dimension": budget.dimension.as_str(),
        "sourceKey": budget.source_key,
        "period": budget.period.as_str(),
        "limitSize": budget.limit_size,
        "warnPercent": budget.warn_percent,
    })
}

/// The platform organization has no notify policies, so nobody would hear of its budgets.
fn refuse_platform_organization(ctx: &IamContext) -> Result<()> {
    if ctx.is_system_scope() {
        return Err(Error::invalid(
            "the platform organization cannot set intake budgets",
        ));
    }

    Ok(())
}

/// A credential that is gone sends nothing more, so its budget would never mean anything.
async fn refuse_missing_source(
    state: &AppState,
    ctx: &IamContext,
    budget: &IntakeBudget,
) -> Result<()> {
    if budget.dimension != UsageDimension::Credential {
        return Ok(());
    }

    let book = state
        .platform
        .intake_sources
        .book(&ctx.org_id, budget.dimension, &[budget.source_key.as_str()])
        .await;

    if book
        .labels
        .get(&budget.source_key)
        .is_some_and(|label| label.missing)
    {
        return Err(Error::invalid(format!(
            "the source `{}` no longer exists",
            budget.source_key
        )));
    }

    Ok(())
}

/// The people to tell, who must be members of the organization: nobody else can be reached through
/// its notify policies. Named nobody, it is the person setting the budget.
async fn recipients(
    state: &AppState,
    ctx: &IamContext,
    requested: Option<Vec<String>>,
) -> Result<Vec<Id>> {
    let requested = requested.unwrap_or_default();

    if requested.is_empty() && ctx.credential_service_account_id.is_some() {
        return Err(Error::invalid(
            "a service account must name who is told about a budget",
        ));
    }

    let requested = if requested.is_empty() {
        vec![ctx.user_id.to_string()]
    } else {
        requested
    };

    let members = state
        .iam
        .service
        .iam_memberships
        .list_for_org(&ctx.org_id)
        .await?
        .into_iter()
        .map(|membership| membership.user_id)
        .collect::<HashSet<_>>();

    let mut ids = Vec::with_capacity(requested.len());

    for user_id in requested {
        let id = Id::from_string(user_id);

        if !members.contains(&id) {
            return Err(Error::invalid(format!(
                "`{id}` is not a member of this organization"
            )));
        }

        if !ids.contains(&id) {
            ids.push(id);
        }
    }

    Ok(ids)
}
