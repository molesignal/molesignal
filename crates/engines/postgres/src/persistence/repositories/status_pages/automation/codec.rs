// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sqlx::Row as _;

use crate::{
    domain::status_page::{
        ActiveAutomationRule, AutomationCandidate, AutomationCandidateState, AutomationOutboxItem,
        AutomationRuleLifecycle, IncidentImpact, StatusAutomationRevision, StatusAutomationRule,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) const ACTIVE_RULE_COLS: &str = "
    rule.id AS rule_id, rule.organization_id, rule.status_page_id, rule.name,
    rule.position, rule.lifecycle, rule.active_revision_id, rule.draft_revision_id,
    rule.created_by AS rule_created_by, rule.created_at_micros AS rule_created_at_micros,
    rule.updated_at_micros,
    revision.id AS revision_id, revision.number, revision.matchers, revision.action,
    revision.content_hash, revision.created_by AS revision_created_by,
    revision.created_at_micros AS revision_created_at_micros";

pub(super) const CANDIDATE_COLS: &str = "id, organization_id, status_page_id,
    rule_revision_id, correlation_key, state, title, message, resolved_message, impact,
    component_ids, automatic, status_incident_id, due_at_micros, created_at_micros,
    updated_at_micros, last_error";

pub(super) const CANDIDATE_SELECT_COLS: &str = "candidate.id, candidate.organization_id,
    candidate.status_page_id, candidate.rule_revision_id, candidate.correlation_key,
    candidate.state, candidate.title, candidate.message, candidate.resolved_message,
    candidate.impact, candidate.component_ids, candidate.automatic,
    candidate.status_incident_id, candidate.due_at_micros, candidate.created_at_micros,
    candidate.updated_at_micros, candidate.last_error";

pub(super) const OUTBOX_COLS: &str = "id, organization_id, candidate_id, kind,
    idempotency_key, payload, attempts, available_at_micros, created_at_micros";

pub(super) fn row_to_active(row: sqlx::postgres::PgRow) -> Result<ActiveAutomationRule> {
    let lifecycle: String = row.try_get("lifecycle").map_err(super::super::sqlx_err)?;
    Ok(ActiveAutomationRule {
        rule: StatusAutomationRule {
            id: Id(row.try_get("rule_id").map_err(super::super::sqlx_err)?),
            organization_id: Id(row
                .try_get("organization_id")
                .map_err(super::super::sqlx_err)?),
            status_page_id: Id(row
                .try_get("status_page_id")
                .map_err(super::super::sqlx_err)?),
            name: row.try_get("name").map_err(super::super::sqlx_err)?,
            position: row.try_get("position").map_err(super::super::sqlx_err)?,
            lifecycle: AutomationRuleLifecycle::parse(&lifecycle).ok_or_else(|| {
                Error::internal(format!(
                    "unknown Status Page automation lifecycle: {lifecycle}"
                ))
            })?,
            active_revision_id: row
                .try_get::<Option<String>, _>("active_revision_id")
                .map_err(super::super::sqlx_err)?
                .map(Id),
            draft_revision_id: row
                .try_get::<Option<String>, _>("draft_revision_id")
                .map_err(super::super::sqlx_err)?
                .map(Id),
            created_by: Id(row
                .try_get("rule_created_by")
                .map_err(super::super::sqlx_err)?),
            created_at: TimestampMicros(
                row.try_get("rule_created_at_micros")
                    .map_err(super::super::sqlx_err)?,
            ),
            updated_at: TimestampMicros(
                row.try_get("updated_at_micros")
                    .map_err(super::super::sqlx_err)?,
            ),
        },
        revision: StatusAutomationRevision {
            id: Id(row.try_get("revision_id").map_err(super::super::sqlx_err)?),
            organization_id: Id(row
                .try_get("organization_id")
                .map_err(super::super::sqlx_err)?),
            status_page_id: Id(row
                .try_get("status_page_id")
                .map_err(super::super::sqlx_err)?),
            rule_id: Id(row.try_get("rule_id").map_err(super::super::sqlx_err)?),
            number: row
                .try_get::<i32, _>("number")
                .map_err(super::super::sqlx_err)? as u32,
            matchers: row
                .try_get::<sqlx::types::Json<_>, _>("matchers")
                .map_err(super::super::sqlx_err)?
                .0,
            action: row
                .try_get::<sqlx::types::Json<_>, _>("action")
                .map_err(super::super::sqlx_err)?
                .0,
            content_hash: row
                .try_get("content_hash")
                .map_err(super::super::sqlx_err)?,
            created_by: Id(row
                .try_get("revision_created_by")
                .map_err(super::super::sqlx_err)?),
            created_at: TimestampMicros(
                row.try_get("revision_created_at_micros")
                    .map_err(super::super::sqlx_err)?,
            ),
        },
    })
}

pub(super) fn row_to_candidate(row: sqlx::postgres::PgRow) -> Result<AutomationCandidate> {
    let state: String = row.try_get("state").map_err(super::super::sqlx_err)?;
    let impact: String = row.try_get("impact").map_err(super::super::sqlx_err)?;
    Ok(AutomationCandidate {
        id: Id(row.try_get("id").map_err(super::super::sqlx_err)?),
        organization_id: Id(row
            .try_get("organization_id")
            .map_err(super::super::sqlx_err)?),
        status_page_id: Id(row
            .try_get("status_page_id")
            .map_err(super::super::sqlx_err)?),
        rule_revision_id: Id(row
            .try_get("rule_revision_id")
            .map_err(super::super::sqlx_err)?),
        correlation_key: row
            .try_get("correlation_key")
            .map_err(super::super::sqlx_err)?,
        state: AutomationCandidateState::parse(&state).ok_or_else(|| {
            Error::internal(format!("unknown automation Candidate state: {state}"))
        })?,
        title: row.try_get("title").map_err(super::super::sqlx_err)?,
        message: row.try_get("message").map_err(super::super::sqlx_err)?,
        resolved_message: row
            .try_get("resolved_message")
            .map_err(super::super::sqlx_err)?,
        impact: IncidentImpact::parse(&impact)
            .ok_or_else(|| Error::internal(format!("unknown automation impact: {impact}")))?,
        component_ids: row
            .try_get::<Vec<String>, _>("component_ids")
            .map_err(super::super::sqlx_err)?
            .into_iter()
            .map(Id)
            .collect(),
        automatic: row.try_get("automatic").map_err(super::super::sqlx_err)?,
        status_incident_id: row
            .try_get::<Option<String>, _>("status_incident_id")
            .map_err(super::super::sqlx_err)?
            .map(Id),
        due_at: TimestampMicros(
            row.try_get("due_at_micros")
                .map_err(super::super::sqlx_err)?,
        ),
        created_at: TimestampMicros(
            row.try_get("created_at_micros")
                .map_err(super::super::sqlx_err)?,
        ),
        updated_at: TimestampMicros(
            row.try_get("updated_at_micros")
                .map_err(super::super::sqlx_err)?,
        ),
        last_error: row.try_get("last_error").map_err(super::super::sqlx_err)?,
    })
}

pub(super) fn row_to_outbox(row: sqlx::postgres::PgRow) -> Result<AutomationOutboxItem> {
    Ok(AutomationOutboxItem {
        id: Id(row.try_get("id").map_err(super::super::sqlx_err)?),
        organization_id: Id(row
            .try_get("organization_id")
            .map_err(super::super::sqlx_err)?),
        candidate_id: Id(row
            .try_get("candidate_id")
            .map_err(super::super::sqlx_err)?),
        kind: row.try_get("kind").map_err(super::super::sqlx_err)?,
        idempotency_key: row
            .try_get("idempotency_key")
            .map_err(super::super::sqlx_err)?,
        payload: row
            .try_get::<sqlx::types::Json<serde_json::Value>, _>("payload")
            .map_err(super::super::sqlx_err)?
            .0,
        attempts: row
            .try_get::<i32, _>("attempts")
            .map_err(super::super::sqlx_err)? as u32,
        available_at: TimestampMicros(
            row.try_get("available_at_micros")
                .map_err(super::super::sqlx_err)?,
        ),
        created_at: TimestampMicros(
            row.try_get("created_at_micros")
                .map_err(super::super::sqlx_err)?,
        ),
    })
}
