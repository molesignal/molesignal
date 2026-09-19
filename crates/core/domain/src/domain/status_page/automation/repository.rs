// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{
    ActiveAutomationRule, AutomationCandidate, AutomationCandidateDetail, AutomationCandidateState,
    AutomationOutboxItem, AutomationSourceObservation, StatusAutomationRevision,
    StatusAutomationRule,
};
use crate::{
    domain::status_page::StatusPageIncidentUpdate,
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
pub trait StatusPageAutomationRepository: Send + Sync {
    async fn create_automation_rule(
        &self,
        rule: StatusAutomationRule,
        revision: StatusAutomationRevision,
    ) -> Result<ActiveAutomationRule>;
    async fn create_automation_revision(
        &self,
        revision: StatusAutomationRevision,
        rule_name: &str,
        position: Option<i32>,
    ) -> Result<StatusAutomationRevision>;
    async fn activate_automation_revision(
        &self,
        org_id: &Id,
        page_id: &Id,
        rule_id: &Id,
        revision_id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<ActiveAutomationRule>;
    async fn update_automation_rule_lifecycle(
        &self,
        org_id: &Id,
        page_id: &Id,
        rule_id: &Id,
        lifecycle: super::AutomationRuleLifecycle,
        updated_at: TimestampMicros,
    ) -> Result<StatusAutomationRule>;
    async fn list_automation_rules(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Vec<ActiveAutomationRule>>;
    async fn reorder_automation_rules(
        &self,
        org_id: &Id,
        page_id: &Id,
        rule_ids: &[Id],
        updated_at: TimestampMicros,
    ) -> Result<Vec<ActiveAutomationRule>>;
    async fn list_matching_automation_rules(
        &self,
        observation: &AutomationSourceObservation,
    ) -> Result<Vec<ActiveAutomationRule>>;
    async fn observe_automation_source(
        &self,
        candidate: AutomationCandidate,
        observation: AutomationSourceObservation,
        automatic: bool,
    ) -> Result<AutomationCandidate>;
    async fn recover_automation_source(
        &self,
        observation: AutomationSourceObservation,
    ) -> Result<Vec<AutomationCandidate>>;
    async fn list_due_automation_candidates(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<AutomationCandidate>>;
    async fn get_automation_candidate(
        &self,
        org_id: &Id,
        candidate_id: &Id,
    ) -> Result<AutomationCandidate>;
    async fn get_automation_candidate_detail(
        &self,
        org_id: &Id,
        candidate_id: &Id,
    ) -> Result<AutomationCandidateDetail>;
    async fn list_automation_candidates(
        &self,
        org_id: &Id,
        page_id: &Id,
        state: Option<AutomationCandidateState>,
        limit: u32,
    ) -> Result<Vec<AutomationCandidate>>;
    async fn list_pending_automation_candidates(
        &self,
        org_id: &Id,
        limit: u32,
    ) -> Result<Vec<AutomationCandidate>>;
    #[allow(clippy::too_many_arguments)]
    async fn mark_automation_candidate(
        &self,
        org_id: &Id,
        candidate_id: &Id,
        expected: &[AutomationCandidateState],
        state: AutomationCandidateState,
        incident_id: Option<&Id>,
        actor_id: Option<&Id>,
        note: Option<&str>,
        updated_at: TimestampMicros,
    ) -> Result<AutomationCandidate>;
    async fn enqueue_automation_outbox(
        &self,
        candidate: &AutomationCandidate,
        item: AutomationOutboxItem,
    ) -> Result<AutomationOutboxItem>;
    async fn claim_automation_outbox(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<AutomationOutboxItem>>;
    async fn complete_automation_outbox(
        &self,
        item_id: &Id,
        completed_at: TimestampMicros,
    ) -> Result<()>;
    async fn fail_automation_outbox(
        &self,
        item_id: &Id,
        error: &str,
        retry_at: TimestampMicros,
    ) -> Result<bool>;
    async fn retry_automation_candidate(
        &self,
        org_id: &Id,
        candidate_id: &Id,
        actor_id: &Id,
        retried_at: TimestampMicros,
    ) -> Result<AutomationCandidate>;
    async fn resolve_automation_candidate_if_inactive(
        &self,
        org_id: &Id,
        candidate_id: &Id,
        update: StatusPageIncidentUpdate,
    ) -> Result<Option<AutomationCandidate>>;
    async fn get_automation_settings(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Option<super::AutomationSettings>>;
    async fn set_automation_paused(
        &self,
        org_id: &Id,
        page_id: &Id,
        paused: bool,
        actor_id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<super::AutomationSettings>;
}
