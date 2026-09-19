// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{
    super::PgStatusPageRepository,
    codec::{OUTBOX_COLS, row_to_outbox},
};
use crate::{
    domain::status_page::{AutomationCandidate, AutomationOutboxItem},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn enqueue(
    repository: &PgStatusPageRepository,
    candidate: &AutomationCandidate,
    item: AutomationOutboxItem,
) -> Result<AutomationOutboxItem> {
    if candidate.organization_id != item.organization_id || candidate.id != item.candidate_id {
        return Err(Error::invalid(
            "automation Outbox item must belong to its Candidate",
        ));
    }
    let sql = format!(
        "INSERT INTO status_page_automation_outbox
            (id,organization_id,candidate_id,kind,idempotency_key,payload,status,attempts,
             available_at_micros,locked_at_micros,last_error,completed_at_micros,
             created_at_micros,updated_at_micros)
         VALUES ($1,$2,$3,$4,$5,$6,'pending',0,$7,NULL,NULL,NULL,$8,$8)
         ON CONFLICT (idempotency_key) DO UPDATE
             SET id=status_page_automation_outbox.id
         RETURNING {OUTBOX_COLS}"
    );
    let row = sqlx::query(&sql)
        .bind(&item.id.0)
        .bind(&item.organization_id.0)
        .bind(&item.candidate_id.0)
        .bind(&item.kind)
        .bind(&item.idempotency_key)
        .bind(sqlx::types::Json(&item.payload))
        .bind(item.available_at.0)
        .bind(item.created_at.0)
        .fetch_one(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;
    row_to_outbox(row)
}

pub(super) async fn claim(
    repository: &PgStatusPageRepository,
    now: TimestampMicros,
    limit: u32,
) -> Result<Vec<AutomationOutboxItem>> {
    let stale = now.0.saturating_sub(5 * 60 * 1_000_000);
    let sql = format!(
        "WITH candidates AS (
             SELECT item.id AS outbox_id
             FROM status_page_automation_outbox item
             JOIN status_page_automation_candidates candidate
               ON candidate.organization_id=item.organization_id
              AND candidate.id=item.candidate_id
             JOIN status_page_automation_revisions revision
               ON revision.organization_id=candidate.organization_id
              AND revision.id=candidate.rule_revision_id
             JOIN status_page_automation_rules rule
               ON rule.organization_id=revision.organization_id AND rule.id=revision.rule_id
             LEFT JOIN status_page_automation_settings settings
               ON settings.organization_id=candidate.organization_id
              AND settings.status_page_id=candidate.status_page_id
             WHERE ((item.status='pending' AND item.available_at_micros <= $1)
                 OR (item.status='processing' AND item.locked_at_micros < $2))
               AND (item.kind='resolve'
                    OR (rule.lifecycle='active' AND COALESCE(settings.paused,FALSE)=FALSE))
             ORDER BY item.available_at_micros,item.created_at_micros,item.id
             LIMIT $3 FOR UPDATE OF item SKIP LOCKED
         )
         UPDATE status_page_automation_outbox item
         SET status='processing',attempts=item.attempts+1,locked_at_micros=$1,
             updated_at_micros=$1
         FROM candidates WHERE item.id=candidates.outbox_id
         RETURNING {OUTBOX_COLS}"
    );
    sqlx::query(&sql)
        .bind(now.0)
        .bind(stale)
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?
        .into_iter()
        .map(row_to_outbox)
        .collect()
}

pub(super) async fn complete(
    repository: &PgStatusPageRepository,
    item_id: &Id,
    completed_at: TimestampMicros,
) -> Result<()> {
    let rows = sqlx::query(
        "UPDATE status_page_automation_outbox
         SET status='completed',completed_at_micros=$2,locked_at_micros=NULL,
             last_error=NULL,updated_at_micros=$2
         WHERE id=$1 AND status='processing'",
    )
    .bind(&item_id.0)
    .bind(completed_at.0)
    .execute(&repository.pool)
    .await
    .map_err(super::super::sqlx_err)?
    .rows_affected();
    if rows == 0 {
        return Err(Error::conflict("automation Outbox claim was lost"));
    }
    Ok(())
}

pub(super) async fn fail(
    repository: &PgStatusPageRepository,
    item_id: &Id,
    error: &str,
    retry_at: TimestampMicros,
) -> Result<bool> {
    let mut tx = sqlx::begin(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;
    let item: Option<(String, String, String)> = sqlx::query_as(
        "UPDATE status_page_automation_outbox
         SET status=CASE WHEN attempts >= 10 THEN 'dead_letter' ELSE 'pending' END,
             available_at_micros=$3,locked_at_micros=NULL,last_error=$2,
             updated_at_micros=$3
         WHERE id=$1 AND status='processing'
         RETURNING status,organization_id,candidate_id",
    )
    .bind(&item_id.0)
    .bind(truncate(error, 4000))
    .bind(retry_at.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    let Some((status, org_id, candidate_id)) = item else {
        return Err(Error::conflict("automation Outbox claim was lost"));
    };
    if status == "dead_letter" {
        let rows = sqlx::query(
            "UPDATE status_page_automation_candidates
             SET state='failed',last_error=$3,updated_at_micros=$4
             WHERE organization_id=$1 AND id=$2
               AND state IN ('delayed','pending_approval','approved','published','failed')",
        )
        .bind(&org_id)
        .bind(&candidate_id)
        .bind(truncate(error, 4000))
        .bind(retry_at.0)
        .execute(&mut *tx)
        .await
        .map_err(super::super::sqlx_err)?
        .rows_affected();
        if rows > 0 {
            sqlx::query(
                "INSERT INTO status_page_automation_actions
                    (id,organization_id,candidate_id,action,actor_id,note,created_at_micros)
                 VALUES ($1,$2,$3,'failed',NULL,$4,$5)",
            )
            .bind(Id::new().0)
            .bind(&org_id)
            .bind(&candidate_id)
            .bind(truncate(error, 4000))
            .bind(retry_at.0)
            .execute(&mut *tx)
            .await
            .map_err(super::super::sqlx_err)?;
        }
    }
    tx.commit().await.map_err(super::super::sqlx_err)?;
    Ok(status == "dead_letter")
}

pub(super) async fn retry_candidate(
    repository: &PgStatusPageRepository,
    org_id: &Id,
    candidate_id: &Id,
    actor_id: &Id,
    retried_at: TimestampMicros,
) -> Result<AutomationCandidate> {
    let mut tx = sqlx::begin(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;
    let state: Option<String> = sqlx::query_scalar(
        "SELECT state FROM status_page_automation_candidates
         WHERE organization_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    match state.as_deref() {
        None => return Err(Error::not_found("Status Page automation Candidate")),
        Some("failed") => {}
        Some(_) => {
            return Err(Error::conflict(
                "only a failed automation Candidate can be retried",
            ));
        }
    }
    let kinds: Vec<String> = sqlx::query_scalar(
        "UPDATE status_page_automation_outbox
         SET status='pending',attempts=0,available_at_micros=$3,locked_at_micros=NULL,
             last_error=NULL,completed_at_micros=NULL,updated_at_micros=$3
         WHERE organization_id=$1 AND candidate_id=$2 AND status='dead_letter'
         RETURNING kind",
    )
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .bind(retried_at.0)
    .fetch_all(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    if kinds.is_empty() {
        return Err(Error::conflict(
            "automation Candidate has no dead-letter work to retry",
        ));
    }
    let resumed_state = if kinds.iter().any(|kind| kind == "resolve") {
        "published"
    } else if kinds.iter().any(|kind| kind == "publish") {
        "approved"
    } else {
        "pending_approval"
    };
    sqlx::query(
        "UPDATE status_page_automation_candidates
         SET state=$3,last_error=NULL,updated_at_micros=$4
         WHERE organization_id=$1 AND id=$2 AND state='failed'",
    )
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .bind(resumed_state)
    .bind(retried_at.0)
    .execute(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    sqlx::query(
        "INSERT INTO status_page_automation_actions
            (id,organization_id,candidate_id,action,actor_id,note,created_at_micros)
         VALUES ($1,$2,$3,'retried',$4,'dead-letter work retried',$5)",
    )
    .bind(Id::new().0)
    .bind(&org_id.0)
    .bind(&candidate_id.0)
    .bind(&actor_id.0)
    .bind(retried_at.0)
    .execute(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    tx.commit().await.map_err(super::super::sqlx_err)?;
    super::candidates::get(repository, org_id, candidate_id).await
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}
