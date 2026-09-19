// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{
    super::PgStatusPageRepository,
    codec::{CANDIDATE_COLS, row_to_candidate},
};
use crate::{
    domain::status_page::AutomationCandidate,
    shared::{Result, ids::Id},
};

pub(super) async fn list(
    repository: &PgStatusPageRepository,
    org_id: &Id,
    limit: u32,
) -> Result<Vec<AutomationCandidate>> {
    let sql = format!(
        "SELECT {CANDIDATE_COLS} FROM status_page_automation_candidates candidate
         WHERE candidate.organization_id=$1
           AND candidate.state IN ('pending_approval','failed')
         ORDER BY candidate.updated_at_micros DESC,candidate.id DESC LIMIT $2"
    );
    sqlx::query(&sql)
        .bind(&org_id.0)
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?
        .into_iter()
        .map(row_to_candidate)
        .collect()
}
