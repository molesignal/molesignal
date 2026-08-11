// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{
    super::{PgStatusPageRepository, incidents::insert_update, touch_page},
    codec::{CANDIDATE_COLS, row_to_candidate},
};
use crate::{
    domain::status_page::{AutomationCandidate, StatusPageIncidentUpdate},
    shared::{Error, Result, ids::Id},
};

pub(super) async fn resolve_if_inactive(
    repository: &PgStatusPageRepository,
    org_id: &Id,
    candidate_id: &Id,
    update: StatusPageIncidentUpdate,
) -> Result<Option<AutomationCandidate>> {
    if &update.org_id != org_id {
        return Err(Error::invalid(
            "automation resolution update must belong to its Organization",
        ));
    }
    let mut tx = sqlx::begin(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;
    let locked: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT state,status_page_id,status_incident_id
         FROM status_page_automation_candidates
         WHERE organization_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    let Some((state, page_id, incident_id)) = locked else {
        return Err(Error::not_found("Status Page automation Candidate"));
    };
    if !matches!(state.as_str(), "published" | "failed") {
        tx.rollback().await.map_err(super::super::sqlx_err)?;
        return Ok(None);
    }
    let incident_id = incident_id
        .ok_or_else(|| Error::internal("published automation Candidate has no Incident"))?;
    if update.status_page_id.as_str() != page_id.as_str()
        || update.incident_id.as_str() != incident_id.as_str()
    {
        return Err(Error::invalid(
            "automation resolution update must belong to its public Incident",
        ));
    }
    let source_active: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM status_page_automation_candidate_sources
             WHERE organization_id=$1 AND candidate_id=$2 AND active AND NOT muted
         )",
    )
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .fetch_one(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    if source_active {
        tx.rollback().await.map_err(super::super::sqlx_err)?;
        return Ok(None);
    }

    let incident_rows = sqlx::query(
        "UPDATE status_page_incidents
         SET status='resolved',ended_at_micros=$4,updated_at_micros=$4
         WHERE org_id=$1 AND status_page_id=$2 AND id=$3
           AND publication_state='published' AND status <> 'resolved'",
    )
    .bind(&org_id.0)
    .bind(&page_id)
    .bind(&incident_id)
    .bind(update.created_at.0)
    .execute(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?
    .rows_affected();
    if incident_rows != 1 {
        return Err(Error::conflict(
            "public Incident is no longer eligible for automation resolution",
        ));
    }
    insert_update(&mut tx, &update)
        .await
        .map_err(super::super::sqlx_err)?;
    touch_page(&mut tx, org_id, &update.status_page_id, update.created_at).await?;
    let sql = format!(
        "UPDATE status_page_automation_candidates
         SET state='resolved',last_error=NULL,updated_at_micros=$3
         WHERE organization_id=$1 AND id=$2
         RETURNING {CANDIDATE_COLS}"
    );
    let candidate_row = sqlx::query(&sql)
        .bind(&org_id.0)
        .bind(&candidate_id.0)
        .bind(update.created_at.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(super::super::sqlx_err)?;
    sqlx::query(
        "INSERT INTO status_page_automation_actions
            (id,organization_id,candidate_id,action,actor_id,note,created_at_micros)
         VALUES ($1,$2,$3,'resolved',NULL,'all correlated sources recovered',$4)",
    )
    .bind(Id::new().0)
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .bind(update.created_at.0)
    .execute(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    tx.commit().await.map_err(super::super::sqlx_err)?;
    row_to_candidate(candidate_row).map(Some)
}
