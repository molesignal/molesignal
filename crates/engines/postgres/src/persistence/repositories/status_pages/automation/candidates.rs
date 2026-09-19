// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sqlx::Row as _;

use super::{
    super::PgStatusPageRepository,
    codec::{CANDIDATE_COLS, CANDIDATE_SELECT_COLS, row_to_candidate},
};
use crate::{
    domain::{
        alerting::incident::Severity,
        status_page::{
            AutomationCandidate, AutomationCandidateAction, AutomationCandidateDetail,
            AutomationCandidateSource, AutomationCandidateState, AutomationSourceKind,
            AutomationSourceObservation, AutomationWorkItem,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn observe(
    repository: &PgStatusPageRepository,
    candidate: AutomationCandidate,
    observation: AutomationSourceObservation,
    automatic: bool,
) -> Result<AutomationCandidate> {
    if !observation.active {
        return Err(Error::invalid(
            "active source observation is required when creating an automation Candidate",
        ));
    }
    let mut tx = sqlx::begin(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;
    let sql = format!(
        "INSERT INTO status_page_automation_candidates
            (id,organization_id,status_page_id,rule_revision_id,correlation_key,state,
             title,message,resolved_message,impact,component_ids,automatic,status_incident_id,
             due_at_micros,last_error,created_at_micros,updated_at_micros)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,NULL,$13,NULL,$14,$15)
         ON CONFLICT (organization_id,status_page_id,correlation_key)
           WHERE state IN ('delayed','pending_approval','approved','published','failed')
         DO UPDATE SET
            title=EXCLUDED.title, message=EXCLUDED.message,
            resolved_message=EXCLUDED.resolved_message,
            impact=EXCLUDED.impact, component_ids=EXCLUDED.component_ids,
            updated_at_micros=GREATEST(status_page_automation_candidates.updated_at_micros,
                                      EXCLUDED.updated_at_micros)
         RETURNING {CANDIDATE_COLS}"
    );
    let row = sqlx::query(&sql)
        .bind(&candidate.id.0)
        .bind(&candidate.organization_id.0)
        .bind(&candidate.status_page_id.0)
        .bind(&candidate.rule_revision_id.0)
        .bind(&candidate.correlation_key)
        .bind(candidate.state.as_str())
        .bind(&candidate.title)
        .bind(&candidate.message)
        .bind(&candidate.resolved_message)
        .bind(candidate.impact.as_str())
        .bind(
            candidate
                .component_ids
                .iter()
                .map(Id::as_str)
                .collect::<Vec<_>>(),
        )
        .bind(automatic)
        .bind(candidate.due_at.0)
        .bind(candidate.created_at.0)
        .bind(candidate.updated_at.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(super::super::sqlx_err)?;
    let persisted = row_to_candidate(row)?;
    sqlx::query(
        "INSERT INTO status_page_automation_candidate_sources
            (organization_id,candidate_id,source_kind,source_id,source_instance_id,severity,
             labels,active,muted,observed_at_micros,updated_at_micros)
         VALUES ($1,$2,$3,$4,$5,$6,$7,TRUE,$8,$9,$9)
         ON CONFLICT (organization_id,candidate_id,source_kind,source_instance_id)
         DO UPDATE SET severity=EXCLUDED.severity,labels=EXCLUDED.labels,active=TRUE,
             muted=EXCLUDED.muted,observed_at_micros=EXCLUDED.observed_at_micros,
             updated_at_micros=EXCLUDED.updated_at_micros",
    )
    .bind(&observation.organization_id.0)
    .bind(&persisted.id.0)
    .bind(observation.source_kind.as_str())
    .bind(&observation.source_id.0)
    .bind(&observation.source_instance_id.0)
    .bind(observation.severity.as_str())
    .bind(sqlx::types::Json(&observation.labels))
    .bind(observation.muted)
    .bind(observation.observed_at.0)
    .execute(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    if persisted.id == candidate.id {
        sqlx::query(
            "INSERT INTO status_page_automation_actions
                (id,organization_id,candidate_id,action,actor_id,note,created_at_micros)
             VALUES ($1,$2,$3,'created',NULL,'source matched active Rule',$4)",
        )
        .bind(Id::new().0)
        .bind(&observation.organization_id.0)
        .bind(&persisted.id.0)
        .bind(observation.observed_at.0)
        .execute(&mut *tx)
        .await
        .map_err(super::super::sqlx_err)?;
    }
    if observation.muted {
        cancel_if_inactive(
            &mut tx,
            &observation.organization_id,
            &persisted.id,
            observation.observed_at,
        )
        .await?;
    }
    tx.commit().await.map_err(super::super::sqlx_err)?;
    fetch(repository, &observation.organization_id, &persisted.id).await
}

pub(super) async fn recover(
    repository: &PgStatusPageRepository,
    observation: AutomationSourceObservation,
) -> Result<Vec<AutomationCandidate>> {
    let mut tx = sqlx::begin(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;
    let candidate_ids: Vec<String> = sqlx::query_scalar(
        "UPDATE status_page_automation_candidate_sources source
         SET active=FALSE,muted=$4,observed_at_micros=$5,updated_at_micros=$5
         FROM status_page_automation_candidates candidate
         WHERE source.organization_id=$1 AND source.source_kind=$2
           AND source.source_instance_id=$3
           AND candidate.organization_id=source.organization_id
           AND candidate.id=source.candidate_id
           AND candidate.state IN ('delayed','pending_approval','approved','published','failed')
         RETURNING source.candidate_id",
    )
    .bind(&observation.organization_id.0)
    .bind(observation.source_kind.as_str())
    .bind(&observation.source_instance_id.0)
    .bind(observation.muted)
    .bind(observation.observed_at.0)
    .fetch_all(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    for candidate_id in &candidate_ids {
        cancel_if_inactive(
            &mut tx,
            &observation.organization_id,
            &Id(candidate_id.clone()),
            observation.observed_at,
        )
        .await?;
    }
    tx.commit().await.map_err(super::super::sqlx_err)?;
    let mut candidates = Vec::new();
    for candidate_id in candidate_ids {
        let has_active_source: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM status_page_automation_candidate_sources
                 WHERE organization_id=$1 AND candidate_id=$2
                   AND active AND NOT muted
             )",
        )
        .bind(&observation.organization_id.0)
        .bind(&candidate_id)
        .fetch_one(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;
        if !has_active_source {
            candidates
                .push(fetch(repository, &observation.organization_id, &Id(candidate_id)).await?);
        }
    }
    Ok(candidates)
}

pub(super) async fn list_due(
    repository: &PgStatusPageRepository,
    now: TimestampMicros,
    limit: u32,
) -> Result<Vec<AutomationCandidate>> {
    let sql = format!(
        "SELECT {CANDIDATE_SELECT_COLS} FROM status_page_automation_candidates candidate
         JOIN status_page_automation_revisions revision
           ON revision.organization_id=candidate.organization_id
          AND revision.id=candidate.rule_revision_id
         JOIN status_page_automation_rules rule
           ON rule.organization_id=revision.organization_id AND rule.id=revision.rule_id
         LEFT JOIN status_page_automation_settings settings
           ON settings.organization_id=candidate.organization_id
          AND settings.status_page_id=candidate.status_page_id
         WHERE candidate.state='delayed' AND candidate.due_at_micros <= $1
           AND rule.lifecycle='active' AND COALESCE(settings.paused,FALSE)=FALSE
           AND EXISTS (
             SELECT 1 FROM status_page_automation_candidate_sources source
             WHERE source.organization_id=candidate.organization_id
               AND source.candidate_id=candidate.id AND source.active AND NOT source.muted
           )
         ORDER BY candidate.due_at_micros,candidate.created_at_micros,candidate.id
         LIMIT $2"
    );
    sqlx::query(&sql)
        .bind(now.0)
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?
        .into_iter()
        .map(row_to_candidate)
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn mark(
    repository: &PgStatusPageRepository,
    org_id: &Id,
    candidate_id: &Id,
    expected: &[AutomationCandidateState],
    state: AutomationCandidateState,
    incident_id: Option<&Id>,
    actor_id: Option<&Id>,
    note: Option<&str>,
    updated_at: TimestampMicros,
) -> Result<AutomationCandidate> {
    let expected = expected
        .iter()
        .map(|state| state.as_str())
        .collect::<Vec<_>>();
    let mut tx = sqlx::begin(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;
    let sql = format!(
        "UPDATE status_page_automation_candidates
         SET state=$3,status_incident_id=COALESCE($4,status_incident_id),
             last_error=CASE WHEN $3='failed' THEN $5 ELSE NULL END,updated_at_micros=$6
         WHERE organization_id=$1 AND id=$2 AND state=ANY($7)
         RETURNING {CANDIDATE_COLS}"
    );
    let row = sqlx::query(&sql)
        .bind(&org_id.0)
        .bind(&candidate_id.0)
        .bind(state.as_str())
        .bind(incident_id.map(Id::as_str))
        .bind(note)
        .bind(updated_at.0)
        .bind(expected)
        .fetch_optional(&mut *tx)
        .await
        .map_err(super::super::sqlx_err)?
        .ok_or_else(|| Error::conflict("automation Candidate state changed concurrently"))?;
    let candidate = row_to_candidate(row)?;
    sqlx::query(
        "INSERT INTO status_page_automation_actions
            (id,organization_id,candidate_id,action,actor_id,note,created_at_micros)
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(Id::new().0)
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .bind(state.as_str())
    .bind(actor_id.map(Id::as_str))
    .bind(note)
    .bind(updated_at.0)
    .execute(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    tx.commit().await.map_err(super::super::sqlx_err)?;
    Ok(candidate)
}

async fn cancel_if_inactive(
    tx: &mut sqlx::PgConnection,
    org_id: &Id,
    candidate_id: &Id,
    updated_at: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "UPDATE status_page_automation_candidates candidate
         SET state=CASE WHEN state IN ('delayed','pending_approval','approved','failed')
                        THEN 'cancelled' ELSE state END,
             updated_at_micros=$3
         WHERE candidate.organization_id=$1 AND candidate.id=$2
           AND NOT EXISTS (
             SELECT 1 FROM status_page_automation_candidate_sources source
             WHERE source.organization_id=candidate.organization_id
               AND source.candidate_id=candidate.id AND source.active AND NOT source.muted
           )",
    )
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .bind(updated_at.0)
    .execute(tx)
    .await
    .map_err(super::super::sqlx_err)?;
    Ok(())
}

async fn fetch(
    repository: &PgStatusPageRepository,
    org_id: &Id,
    candidate_id: &Id,
) -> Result<AutomationCandidate> {
    let sql = format!(
        "SELECT {CANDIDATE_COLS} FROM status_page_automation_candidates
         WHERE organization_id=$1 AND id=$2"
    );
    let row = sqlx::query(&sql)
        .bind(&org_id.0)
        .bind(&candidate_id.0)
        .fetch_one(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;
    row_to_candidate(row)
}

pub(super) async fn get(
    repository: &PgStatusPageRepository,
    org_id: &Id,
    candidate_id: &Id,
) -> Result<AutomationCandidate> {
    fetch(repository, org_id, candidate_id).await
}

pub(super) async fn detail(
    repository: &PgStatusPageRepository,
    org_id: &Id,
    candidate_id: &Id,
) -> Result<AutomationCandidateDetail> {
    let candidate = fetch(repository, org_id, candidate_id).await?;
    let source_rows = sqlx::query(
        "SELECT source_kind,source_id,source_instance_id,severity,labels,active,muted,
                observed_at_micros,updated_at_micros
         FROM status_page_automation_candidate_sources
         WHERE organization_id=$1 AND candidate_id=$2
         ORDER BY active DESC,observed_at_micros DESC,source_kind,source_instance_id",
    )
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .fetch_all(&repository.pool)
    .await
    .map_err(super::super::sqlx_err)?;
    let mut sources = Vec::with_capacity(source_rows.len());
    for row in source_rows {
        let source_kind: String = row.try_get("source_kind").map_err(super::super::sqlx_err)?;
        let severity: String = row.try_get("severity").map_err(super::super::sqlx_err)?;
        sources.push(AutomationCandidateSource {
            source_kind: AutomationSourceKind::parse(&source_kind).ok_or_else(|| {
                Error::internal(format!("unknown automation source kind: {source_kind}"))
            })?,
            source_id: Id(row.try_get("source_id").map_err(super::super::sqlx_err)?),
            source_instance_id: Id(row
                .try_get("source_instance_id")
                .map_err(super::super::sqlx_err)?),
            severity: Severity::from_label(&severity).ok_or_else(|| {
                Error::internal(format!("unknown automation source severity: {severity}"))
            })?,
            labels: row
                .try_get::<sqlx::types::Json<_>, _>("labels")
                .map_err(super::super::sqlx_err)?
                .0,
            active: row.try_get("active").map_err(super::super::sqlx_err)?,
            muted: row.try_get("muted").map_err(super::super::sqlx_err)?,
            observed_at: TimestampMicros(
                row.try_get("observed_at_micros")
                    .map_err(super::super::sqlx_err)?,
            ),
            updated_at: TimestampMicros(
                row.try_get("updated_at_micros")
                    .map_err(super::super::sqlx_err)?,
            ),
        });
    }
    let action_rows = sqlx::query(
        "SELECT id,action,actor_id,note,created_at_micros
         FROM status_page_automation_actions
         WHERE organization_id=$1 AND candidate_id=$2
         ORDER BY created_at_micros DESC,id DESC",
    )
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .fetch_all(&repository.pool)
    .await
    .map_err(super::super::sqlx_err)?;
    let actions = action_rows
        .into_iter()
        .map(|row| {
            Ok(AutomationCandidateAction {
                id: Id(row.try_get("id").map_err(super::super::sqlx_err)?),
                action: row.try_get("action").map_err(super::super::sqlx_err)?,
                actor_id: row
                    .try_get::<Option<String>, _>("actor_id")
                    .map_err(super::super::sqlx_err)?
                    .map(Id),
                note: row.try_get("note").map_err(super::super::sqlx_err)?,
                created_at: TimestampMicros(
                    row.try_get("created_at_micros")
                        .map_err(super::super::sqlx_err)?,
                ),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let work_rows = sqlx::query(
        "SELECT id,kind,status,attempts,available_at_micros,last_error,completed_at_micros,
                created_at_micros,updated_at_micros
         FROM status_page_automation_outbox
         WHERE organization_id=$1 AND candidate_id=$2
         ORDER BY created_at_micros DESC,id DESC",
    )
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .fetch_all(&repository.pool)
    .await
    .map_err(super::super::sqlx_err)?;
    let work_items = work_rows
        .into_iter()
        .map(|row| {
            Ok(AutomationWorkItem {
                id: Id(row.try_get("id").map_err(super::super::sqlx_err)?),
                kind: row.try_get("kind").map_err(super::super::sqlx_err)?,
                status: row.try_get("status").map_err(super::super::sqlx_err)?,
                attempts: row
                    .try_get::<i32, _>("attempts")
                    .map_err(super::super::sqlx_err)? as u32,
                available_at: TimestampMicros(
                    row.try_get("available_at_micros")
                        .map_err(super::super::sqlx_err)?,
                ),
                last_error: row.try_get("last_error").map_err(super::super::sqlx_err)?,
                completed_at: row
                    .try_get::<Option<i64>, _>("completed_at_micros")
                    .map_err(super::super::sqlx_err)?
                    .map(TimestampMicros),
                created_at: TimestampMicros(
                    row.try_get("created_at_micros")
                        .map_err(super::super::sqlx_err)?,
                ),
                updated_at: TimestampMicros(
                    row.try_get("updated_at_micros")
                        .map_err(super::super::sqlx_err)?,
                ),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(AutomationCandidateDetail {
        candidate,
        sources,
        actions,
        work_items,
    })
}

pub(super) async fn list(
    repository: &PgStatusPageRepository,
    org_id: &Id,
    page_id: &Id,
    state: Option<AutomationCandidateState>,
    limit: u32,
) -> Result<Vec<AutomationCandidate>> {
    let sql = format!(
        "SELECT {CANDIDATE_COLS} FROM status_page_automation_candidates candidate
         WHERE candidate.organization_id=$1 AND candidate.status_page_id=$2
           AND ($3::TEXT IS NULL OR candidate.state=$3)
         ORDER BY candidate.updated_at_micros DESC,candidate.id DESC LIMIT $4"
    );
    sqlx::query(&sql)
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(state.map(|state| state.as_str()))
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?
        .into_iter()
        .map(row_to_candidate)
        .collect()
}
