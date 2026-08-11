// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use super::super::PgStatusPageRepository;
use crate::{
    domain::status_page::{ActiveAutomationRule, StatusPageAutomationRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn reorder(
    repository: &PgStatusPageRepository,
    org_id: &Id,
    page_id: &Id,
    rule_ids: &[Id],
    updated_at: TimestampMicros,
) -> Result<Vec<ActiveAutomationRule>> {
    let mut tx = sqlx::begin(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;
    let existing: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM status_page_automation_rules
         WHERE organization_id=$1 AND status_page_id=$2 AND lifecycle <> 'archived'
         ORDER BY position,id FOR UPDATE",
    )
    .bind(&org_id.0)
    .bind(&page_id.0)
    .fetch_all(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?;
    let requested = rule_ids
        .iter()
        .map(|id| id.0.clone())
        .collect::<HashSet<_>>();
    if existing.len() != rule_ids.len()
        || requested.len() != rule_ids.len()
        || existing.iter().any(|id| !requested.contains(id))
    {
        return Err(Error::conflict(
            "automation Rule order must contain every active Rule exactly once",
        ));
    }
    let ordered_ids = rule_ids.iter().map(|id| id.0.clone()).collect::<Vec<_>>();
    let rows = sqlx::query(
        "UPDATE status_page_automation_rules rule
         SET position=(ordered.position - 1)::INTEGER, updated_at_micros=$4
         FROM UNNEST($3::TEXT[]) WITH ORDINALITY AS ordered(id, position)
         WHERE rule.organization_id=$1 AND rule.status_page_id=$2
           AND rule.lifecycle <> 'archived' AND rule.id=ordered.id",
    )
    .bind(&org_id.0)
    .bind(&page_id.0)
    .bind(ordered_ids)
    .bind(updated_at.0)
    .execute(&mut *tx)
    .await
    .map_err(super::super::sqlx_err)?
    .rows_affected();
    if rows != rule_ids.len() as u64 {
        return Err(Error::conflict(
            "automation Rule set changed while it was being reordered",
        ));
    }
    tx.commit().await.map_err(super::super::sqlx_err)?;
    repository.list_automation_rules(org_id, page_id).await
}
