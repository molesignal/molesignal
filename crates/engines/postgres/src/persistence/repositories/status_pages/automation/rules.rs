// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{
    super::PgStatusPageRepository,
    codec::{ACTIVE_RULE_COLS, row_to_active},
};
use crate::{
    domain::status_page::{
        ActiveAutomationRule, AutomationRuleLifecycle, AutomationSettings,
        AutomationSourceObservation, StatusAutomationRevision, StatusAutomationRule,
        StatusPageAutomationRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
impl StatusPageAutomationRepository for PgStatusPageRepository {
    async fn create_automation_rule(
        &self,
        rule: StatusAutomationRule,
        revision: StatusAutomationRevision,
    ) -> Result<ActiveAutomationRule> {
        let mut tx = sqlx::begin(&self.pool)
            .await
            .map_err(super::super::sqlx_err)?;
        sqlx::query(
            "INSERT INTO status_page_automation_rules
                (id, organization_id, status_page_id, name, position, lifecycle,
                 active_revision_id, draft_revision_id, created_by, created_at_micros,
                 updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,NULL,$7,$8,$9,$10)",
        )
        .bind(&rule.id.0)
        .bind(&rule.organization_id.0)
        .bind(&rule.status_page_id.0)
        .bind(&rule.name)
        .bind(rule.position)
        .bind(rule.lifecycle.as_str())
        .bind(&revision.id.0)
        .bind(&rule.created_by.0)
        .bind(rule.created_at.0)
        .bind(rule.updated_at.0)
        .execute(&mut *tx)
        .await
        .map_err(super::super::sqlx_err)?;
        insert_revision(&mut tx, &revision).await?;
        tx.commit().await.map_err(super::super::sqlx_err)?;
        self.get_rule_revision(&rule.organization_id, &rule.status_page_id, &rule.id, false)
            .await
    }

    async fn create_automation_revision(
        &self,
        revision: StatusAutomationRevision,
        rule_name: &str,
        position: Option<i32>,
    ) -> Result<StatusAutomationRevision> {
        let mut tx = sqlx::begin(&self.pool)
            .await
            .map_err(super::super::sqlx_err)?;
        insert_revision(&mut tx, &revision).await?;
        let rows = sqlx::query(
            "UPDATE status_page_automation_rules
             SET draft_revision_id=$4, name=$5, position=COALESCE($6, position),
                 updated_at_micros=$7
             WHERE organization_id=$1 AND status_page_id=$2 AND id=$3
               AND lifecycle <> 'archived'",
        )
        .bind(&revision.organization_id.0)
        .bind(&revision.status_page_id.0)
        .bind(&revision.rule_id.0)
        .bind(&revision.id.0)
        .bind(rule_name)
        .bind(position)
        .bind(revision.created_at.0)
        .execute(&mut *tx)
        .await
        .map_err(super::super::sqlx_err)?
        .rows_affected();
        if rows == 0 {
            return Err(Error::not_found("Status Page automation Rule"));
        }
        tx.commit().await.map_err(super::super::sqlx_err)?;
        Ok(revision)
    }

    async fn activate_automation_revision(
        &self,
        org_id: &Id,
        page_id: &Id,
        rule_id: &Id,
        revision_id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<ActiveAutomationRule> {
        let rows = sqlx::query(
            "UPDATE status_page_automation_rules rule
             SET active_revision_id=$4, draft_revision_id=NULL, lifecycle='active',
                 updated_at_micros=$5
             FROM status_page_automation_revisions revision
             WHERE rule.organization_id=$1 AND rule.status_page_id=$2 AND rule.id=$3
               AND revision.organization_id=rule.organization_id
               AND revision.status_page_id=rule.status_page_id
               AND revision.rule_id=rule.id AND revision.id=$4
               AND rule.lifecycle <> 'archived'",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(&rule_id.0)
        .bind(&revision_id.0)
        .bind(updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::sqlx_err)?
        .rows_affected();
        if rows == 0 {
            return Err(Error::not_found("Status Page automation Revision"));
        }
        self.get_rule_revision(org_id, page_id, rule_id, true).await
    }

    async fn update_automation_rule_lifecycle(
        &self,
        org_id: &Id,
        page_id: &Id,
        rule_id: &Id,
        lifecycle: AutomationRuleLifecycle,
        updated_at: TimestampMicros,
    ) -> Result<StatusAutomationRule> {
        sqlx::query(
            "UPDATE status_page_automation_rules
             SET lifecycle=$4, updated_at_micros=$5
             WHERE organization_id=$1 AND status_page_id=$2 AND id=$3",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(&rule_id.0)
        .bind(lifecycle.as_str())
        .bind(updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::sqlx_err)?;
        Ok(self
            .get_rule_revision(
                org_id,
                page_id,
                rule_id,
                lifecycle == AutomationRuleLifecycle::Active,
            )
            .await?
            .rule)
    }

    async fn list_automation_rules(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Vec<ActiveAutomationRule>> {
        let sql = format!(
            "SELECT {ACTIVE_RULE_COLS} FROM status_page_automation_rules rule
             JOIN status_page_automation_revisions revision
               ON revision.organization_id=rule.organization_id
              AND revision.id=COALESCE(rule.active_revision_id, rule.draft_revision_id)
             WHERE rule.organization_id=$1 AND rule.status_page_id=$2
               AND rule.lifecycle <> 'archived'
             ORDER BY rule.position, rule.id"
        );
        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::super::sqlx_err)?
            .into_iter()
            .map(row_to_active)
            .collect()
    }

    async fn reorder_automation_rules(
        &self,
        org_id: &Id,
        page_id: &Id,
        rule_ids: &[Id],
        updated_at: TimestampMicros,
    ) -> Result<Vec<ActiveAutomationRule>> {
        super::order::reorder(self, org_id, page_id, rule_ids, updated_at).await
    }

    async fn list_matching_automation_rules(
        &self,
        observation: &AutomationSourceObservation,
    ) -> Result<Vec<ActiveAutomationRule>> {
        let sql = format!(
            "SELECT {ACTIVE_RULE_COLS} FROM status_page_automation_rules rule
             JOIN status_page_automation_revisions revision
               ON revision.organization_id=rule.organization_id
              AND revision.id=rule.active_revision_id
             LEFT JOIN status_page_automation_settings settings
               ON settings.organization_id=rule.organization_id
              AND settings.status_page_id=rule.status_page_id
             WHERE rule.organization_id=$1 AND rule.lifecycle='active'
               AND COALESCE(settings.paused, FALSE)=FALSE
               AND revision.matchers->>'source_kind'=$2
             ORDER BY rule.status_page_id, rule.position, rule.id"
        );
        sqlx::query(&sql)
            .bind(&observation.organization_id.0)
            .bind(observation.source_kind.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(super::super::sqlx_err)?
            .into_iter()
            .map(row_to_active)
            .collect()
    }

    async fn observe_automation_source(
        &self,
        candidate: crate::domain::status_page::AutomationCandidate,
        observation: AutomationSourceObservation,
        automatic: bool,
    ) -> Result<crate::domain::status_page::AutomationCandidate> {
        super::candidates::observe(self, candidate, observation, automatic).await
    }

    async fn recover_automation_source(
        &self,
        observation: AutomationSourceObservation,
    ) -> Result<Vec<crate::domain::status_page::AutomationCandidate>> {
        super::candidates::recover(self, observation).await
    }

    async fn list_due_automation_candidates(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<crate::domain::status_page::AutomationCandidate>> {
        super::candidates::list_due(self, now, limit).await
    }

    async fn get_automation_candidate(
        &self,
        org_id: &Id,
        candidate_id: &Id,
    ) -> Result<crate::domain::status_page::AutomationCandidate> {
        super::candidates::get(self, org_id, candidate_id).await
    }

    async fn get_automation_candidate_detail(
        &self,
        org_id: &Id,
        candidate_id: &Id,
    ) -> Result<crate::domain::status_page::AutomationCandidateDetail> {
        super::candidates::detail(self, org_id, candidate_id).await
    }

    async fn list_automation_candidates(
        &self,
        org_id: &Id,
        page_id: &Id,
        state: Option<crate::domain::status_page::AutomationCandidateState>,
        limit: u32,
    ) -> Result<Vec<crate::domain::status_page::AutomationCandidate>> {
        super::candidates::list(self, org_id, page_id, state, limit).await
    }

    async fn list_pending_automation_candidates(
        &self,
        org_id: &Id,
        limit: u32,
    ) -> Result<Vec<crate::domain::status_page::AutomationCandidate>> {
        super::pending::list(self, org_id, limit).await
    }

    async fn mark_automation_candidate(
        &self,
        org_id: &Id,
        candidate_id: &Id,
        expected: &[crate::domain::status_page::AutomationCandidateState],
        state: crate::domain::status_page::AutomationCandidateState,
        incident_id: Option<&Id>,
        actor_id: Option<&Id>,
        note: Option<&str>,
        updated_at: TimestampMicros,
    ) -> Result<crate::domain::status_page::AutomationCandidate> {
        super::candidates::mark(
            self,
            org_id,
            candidate_id,
            expected,
            state,
            incident_id,
            actor_id,
            note,
            updated_at,
        )
        .await
    }

    async fn enqueue_automation_outbox(
        &self,
        candidate: &crate::domain::status_page::AutomationCandidate,
        item: crate::domain::status_page::AutomationOutboxItem,
    ) -> Result<crate::domain::status_page::AutomationOutboxItem> {
        super::outbox::enqueue(self, candidate, item).await
    }

    async fn claim_automation_outbox(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<crate::domain::status_page::AutomationOutboxItem>> {
        super::outbox::claim(self, now, limit).await
    }

    async fn complete_automation_outbox(
        &self,
        item_id: &Id,
        completed_at: TimestampMicros,
    ) -> Result<()> {
        super::outbox::complete(self, item_id, completed_at).await
    }

    async fn fail_automation_outbox(
        &self,
        item_id: &Id,
        error: &str,
        retry_at: TimestampMicros,
    ) -> Result<bool> {
        super::outbox::fail(self, item_id, error, retry_at).await
    }

    async fn retry_automation_candidate(
        &self,
        org_id: &Id,
        candidate_id: &Id,
        actor_id: &Id,
        retried_at: TimestampMicros,
    ) -> Result<crate::domain::status_page::AutomationCandidate> {
        super::outbox::retry_candidate(self, org_id, candidate_id, actor_id, retried_at).await
    }

    async fn resolve_automation_candidate_if_inactive(
        &self,
        org_id: &Id,
        candidate_id: &Id,
        update: crate::domain::status_page::StatusPageIncidentUpdate,
    ) -> Result<Option<crate::domain::status_page::AutomationCandidate>> {
        super::resolution::resolve_if_inactive(self, org_id, candidate_id, update).await
    }

    async fn get_automation_settings(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Option<AutomationSettings>> {
        let row = sqlx::query(
            "SELECT organization_id,status_page_id,paused,updated_by,updated_at_micros
             FROM status_page_automation_settings
             WHERE organization_id=$1 AND status_page_id=$2",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(super::super::sqlx_err)?;
        row.map(row_to_settings).transpose()
    }

    async fn set_automation_paused(
        &self,
        org_id: &Id,
        page_id: &Id,
        paused: bool,
        actor_id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<AutomationSettings> {
        let row = sqlx::query(
            "INSERT INTO status_page_automation_settings
                (organization_id,status_page_id,paused,updated_by,updated_at_micros)
             VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (organization_id,status_page_id) DO UPDATE
             SET paused=EXCLUDED.paused,updated_by=EXCLUDED.updated_by,
                 updated_at_micros=EXCLUDED.updated_at_micros
             RETURNING organization_id,status_page_id,paused,updated_by,updated_at_micros",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(paused)
        .bind(&actor_id.0)
        .bind(updated_at.0)
        .fetch_one(&self.pool)
        .await
        .map_err(super::super::sqlx_err)?;
        row_to_settings(row)
    }
}

fn row_to_settings(row: sqlx::postgres::PgRow) -> Result<AutomationSettings> {
    use sqlx::Row;

    Ok(AutomationSettings {
        organization_id: Id(row
            .try_get("organization_id")
            .map_err(super::super::sqlx_err)?),
        status_page_id: Id(row
            .try_get("status_page_id")
            .map_err(super::super::sqlx_err)?),
        paused: row.try_get("paused").map_err(super::super::sqlx_err)?,
        updated_by: Id(row.try_get("updated_by").map_err(super::super::sqlx_err)?),
        updated_at: TimestampMicros(
            row.try_get("updated_at_micros")
                .map_err(super::super::sqlx_err)?,
        ),
    })
}

async fn insert_revision(
    tx: &mut sqlx::PgConnection,
    revision: &StatusAutomationRevision,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO status_page_automation_revisions
            (id,organization_id,status_page_id,rule_id,number,matchers,action,content_hash,
             created_by,created_at_micros)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(&revision.id.0)
    .bind(&revision.organization_id.0)
    .bind(&revision.status_page_id.0)
    .bind(&revision.rule_id.0)
    .bind(revision.number as i32)
    .bind(sqlx::types::Json(&revision.matchers))
    .bind(sqlx::types::Json(&revision.action))
    .bind(&revision.content_hash)
    .bind(&revision.created_by.0)
    .bind(revision.created_at.0)
    .execute(tx)
    .await
    .map_err(super::super::sqlx_err)?;
    Ok(())
}

impl PgStatusPageRepository {
    async fn get_rule_revision(
        &self,
        org_id: &Id,
        page_id: &Id,
        rule_id: &Id,
        active: bool,
    ) -> Result<ActiveAutomationRule> {
        let revision = if active {
            "rule.active_revision_id"
        } else {
            "COALESCE(rule.draft_revision_id, rule.active_revision_id)"
        };
        let sql = format!(
            "SELECT {ACTIVE_RULE_COLS} FROM status_page_automation_rules rule
             JOIN status_page_automation_revisions revision
               ON revision.organization_id=rule.organization_id AND revision.id={revision}
             WHERE rule.organization_id=$1 AND rule.status_page_id=$2 AND rule.id=$3"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(&rule_id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::super::sqlx_err)?;
        row_to_active(row)
    }
}
