// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use super::{AutomationRuleInput, AutomationSimulation};
use crate::{
    app::status_page::StatusPageService,
    domain::status_page::{
        ActiveAutomationRule, AutomationCandidate, AutomationCandidateDetail,
        AutomationCandidateState, AutomationRuleLifecycle, AutomationSettings,
        AutomationSourceObservation, StatusAutomationRevision, StatusAutomationRule,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl StatusPageService {
    pub async fn create_automation_rule(
        &self,
        org_id: &Id,
        page_id: &Id,
        actor_id: &Id,
        input: AutomationRuleInput,
    ) -> Result<ActiveAutomationRule> {
        self.repository.get_page(org_id, page_id).await?;
        self.validate_automation_input(org_id, page_id, &input)
            .await?;
        let existing = self
            .repository
            .list_automation_rules(org_id, page_id)
            .await?;
        let now = TimestampMicros::now();
        let rule_id = Id::new();
        let revision = materialize_revision(org_id, page_id, actor_id, &rule_id, 1, &input, now)?;
        self.repository
            .create_automation_rule(
                StatusAutomationRule {
                    id: rule_id,
                    organization_id: org_id.clone(),
                    status_page_id: page_id.clone(),
                    name: input.name.trim().to_string(),
                    position: input.position.unwrap_or(existing.len() as i32),
                    lifecycle: AutomationRuleLifecycle::Draft,
                    active_revision_id: None,
                    draft_revision_id: Some(revision.id.clone()),
                    created_by: actor_id.clone(),
                    created_at: now,
                    updated_at: now,
                },
                revision,
            )
            .await
    }

    pub async fn create_automation_revision(
        &self,
        org_id: &Id,
        page_id: &Id,
        rule_id: &Id,
        actor_id: &Id,
        input: AutomationRuleInput,
    ) -> Result<StatusAutomationRevision> {
        self.validate_automation_input(org_id, page_id, &input)
            .await?;
        let existing = self
            .repository
            .list_automation_rules(org_id, page_id)
            .await?;
        let view = existing
            .iter()
            .find(|view| &view.rule.id == rule_id)
            .ok_or_else(|| Error::not_found("Status Page automation Rule"))?;
        let revision = materialize_revision(
            org_id,
            page_id,
            actor_id,
            rule_id,
            view.revision.number.saturating_add(1),
            &input,
            TimestampMicros::now(),
        )?;
        self.repository
            .create_automation_revision(revision, input.name.trim(), input.position)
            .await
    }

    pub async fn activate_automation_revision(
        &self,
        org_id: &Id,
        page_id: &Id,
        rule_id: &Id,
        revision_id: &Id,
    ) -> Result<ActiveAutomationRule> {
        self.repository
            .activate_automation_revision(
                org_id,
                page_id,
                rule_id,
                revision_id,
                TimestampMicros::now(),
            )
            .await
    }

    pub async fn set_automation_rule_lifecycle(
        &self,
        org_id: &Id,
        page_id: &Id,
        rule_id: &Id,
        lifecycle: AutomationRuleLifecycle,
    ) -> Result<StatusAutomationRule> {
        self.repository
            .update_automation_rule_lifecycle(
                org_id,
                page_id,
                rule_id,
                lifecycle,
                TimestampMicros::now(),
            )
            .await
    }

    pub async fn list_automation_rules(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Vec<ActiveAutomationRule>> {
        self.repository.get_page(org_id, page_id).await?;
        self.repository.list_automation_rules(org_id, page_id).await
    }

    pub async fn reorder_automation_rules(
        &self,
        org_id: &Id,
        page_id: &Id,
        rule_ids: &[Id],
    ) -> Result<Vec<ActiveAutomationRule>> {
        self.repository.get_page(org_id, page_id).await?;
        if rule_ids.len() > 500 {
            return Err(Error::invalid(
                "automation Rule order cannot contain more than 500 Rules",
            ));
        }
        self.repository
            .reorder_automation_rules(org_id, page_id, rule_ids, TimestampMicros::now())
            .await
    }

    pub async fn list_automation_candidates(
        &self,
        org_id: &Id,
        page_id: &Id,
        state: Option<AutomationCandidateState>,
        limit: u32,
    ) -> Result<Vec<AutomationCandidate>> {
        self.repository.get_page(org_id, page_id).await?;
        self.repository
            .list_automation_candidates(org_id, page_id, state, limit)
            .await
    }

    pub async fn list_pending_automation_candidates(
        &self,
        org_id: &Id,
        limit: u32,
    ) -> Result<Vec<AutomationCandidate>> {
        self.repository
            .list_pending_automation_candidates(org_id, limit.min(100))
            .await
    }

    pub async fn get_automation_candidate(
        &self,
        org_id: &Id,
        page_id: &Id,
        candidate_id: &Id,
    ) -> Result<AutomationCandidate> {
        let candidate = self
            .repository
            .get_automation_candidate(org_id, candidate_id)
            .await?;
        if &candidate.status_page_id != page_id {
            return Err(Error::not_found("Status Page automation Candidate"));
        }
        Ok(candidate)
    }

    pub async fn get_automation_candidate_detail(
        &self,
        org_id: &Id,
        page_id: &Id,
        candidate_id: &Id,
    ) -> Result<AutomationCandidateDetail> {
        let detail = self
            .repository
            .get_automation_candidate_detail(org_id, candidate_id)
            .await?;
        if &detail.candidate.status_page_id != page_id {
            return Err(Error::not_found("Status Page automation Candidate"));
        }
        Ok(detail)
    }

    pub async fn retry_automation_candidate(
        &self,
        org_id: &Id,
        page_id: &Id,
        candidate_id: &Id,
        actor_id: &Id,
    ) -> Result<AutomationCandidate> {
        self.get_automation_candidate(org_id, page_id, candidate_id)
            .await?;
        self.repository
            .retry_automation_candidate(org_id, candidate_id, actor_id, TimestampMicros::now())
            .await
    }

    pub async fn get_automation_settings(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Option<AutomationSettings>> {
        self.repository.get_page(org_id, page_id).await?;
        self.repository
            .get_automation_settings(org_id, page_id)
            .await
    }

    pub async fn set_automation_paused(
        &self,
        org_id: &Id,
        page_id: &Id,
        actor_id: &Id,
        paused: bool,
    ) -> Result<AutomationSettings> {
        self.repository.get_page(org_id, page_id).await?;
        self.repository
            .set_automation_paused(org_id, page_id, paused, actor_id, TimestampMicros::now())
            .await
    }

    pub async fn simulate_automation(
        &self,
        observation: &AutomationSourceObservation,
        page_id: &Id,
    ) -> Result<AutomationSimulation> {
        let rules = self
            .repository
            .list_matching_automation_rules(observation)
            .await?;
        if let Some(rule) = rules
            .into_iter()
            .filter(|rule| &rule.rule.status_page_id == page_id)
            .find(|rule| complete_match(rule, observation))
        {
            return Ok(AutomationSimulation {
                matched_rule_id: Some(rule.rule.id),
                matched_revision_id: Some(rule.revision.id),
                reason: "first complete ordered match".to_string(),
            });
        }
        Ok(AutomationSimulation {
            matched_rule_id: None,
            matched_revision_id: None,
            reason: "no active Rule matched every condition".to_string(),
        })
    }

    async fn validate_automation_input(
        &self,
        org_id: &Id,
        page_id: &Id,
        input: &AutomationRuleInput,
    ) -> Result<()> {
        if input.name.trim().is_empty() || input.name.chars().count() > 255 {
            return Err(Error::invalid(
                "automation Rule name must contain 1 to 255 characters",
            ));
        }
        if input.position.is_some_and(|position| position < 0) {
            return Err(Error::invalid(
                "automation Rule position cannot be negative",
            ));
        }
        if input.action.component_ids.is_empty() || input.action.component_ids.len() > 500 {
            return Err(Error::invalid(
                "automation Rule must select 1 to 500 Components",
            ));
        }
        if input.action.sustained_delay_seconds > 86_400 {
            return Err(Error::invalid(
                "automation sustained delay cannot exceed 24 hours",
            ));
        }
        if input.action.correlation_key_template.trim().is_empty()
            || input.action.correlation_key_template.chars().count() > 255
        {
            return Err(Error::invalid("automation correlation template is invalid"));
        }
        if input.action.templates.title.trim().is_empty()
            || input.action.templates.title.chars().count() > 200
        {
            return Err(Error::invalid(
                "automation title Template must contain 1 to 200 characters",
            ));
        }
        for template in [
            &input.action.templates.investigating,
            &input.action.templates.update,
            &input.action.templates.resolved,
        ] {
            if template.trim().is_empty() || template.chars().count() > 4000 {
                return Err(Error::invalid(
                    "automation Templates must contain 1 to 4000 characters",
                ));
            }
        }
        for template in [
            &input.action.templates.title,
            &input.action.templates.investigating,
            &input.action.templates.update,
            &input.action.templates.resolved,
            &input.action.correlation_key_template,
        ] {
            validate_template_tokens(template)?;
        }
        let selected = input
            .action
            .component_ids
            .iter()
            .map(Id::as_str)
            .collect::<HashSet<_>>();
        let available = self
            .repository
            .list_components(org_id, page_id)
            .await?
            .into_iter()
            .filter(|component| {
                component.lifecycle == crate::domain::status_page::ComponentLifecycle::Active
            })
            .map(|component| component.id.0)
            .collect::<HashSet<_>>();
        if selected.iter().any(|id| !available.contains(*id)) {
            return Err(Error::not_found("automation Component not found"));
        }
        Ok(())
    }
}

fn validate_template_tokens(template: &str) -> Result<()> {
    let mut remaining = template;
    while let Some(start) = remaining.find("{{") {
        let after = &remaining[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| Error::invalid("automation Template has an unclosed token"))?;
        let token = after[..end].trim();
        let allowed = matches!(
            token,
            "source.id" | "source.kind" | "monitor.id" | "incident.id" | "severity"
        ) || token
            .strip_prefix("labels.")
            .is_some_and(|label| !label.is_empty() && label.len() <= 128);
        if !allowed {
            return Err(Error::invalid(format!(
                "unsupported automation Template token: {token}"
            )));
        }
        remaining = &after[end + 2..];
    }
    if remaining.contains("}}") {
        return Err(Error::invalid(
            "automation Template has a closing token without an opening token",
        ));
    }
    Ok(())
}

pub(super) fn complete_match(
    rule: &ActiveAutomationRule,
    observation: &AutomationSourceObservation,
) -> bool {
    let matchers = &rule.revision.matchers;
    matchers.source_kind == observation.source_kind
        && matchers
            .source_id
            .as_ref()
            .is_none_or(|id| id == &observation.source_id)
        && matchers
            .minimum_severity
            .is_none_or(|minimum| observation.severity >= minimum)
        && matchers
            .labels
            .iter()
            .all(|(key, value)| observation.labels.get(key) == Some(value))
}

fn materialize_revision(
    org_id: &Id,
    page_id: &Id,
    actor_id: &Id,
    rule_id: &Id,
    number: u32,
    input: &AutomationRuleInput,
    now: TimestampMicros,
) -> Result<StatusAutomationRevision> {
    let content = serde_json::to_vec(input)
        .map_err(|error| Error::internal(format!("serialize automation Revision: {error}")))?;
    Ok(StatusAutomationRevision {
        id: Id::new(),
        organization_id: org_id.clone(),
        status_page_id: page_id.clone(),
        rule_id: rule_id.clone(),
        number,
        matchers: input.matchers.clone(),
        action: input.action.clone(),
        content_hash: blake3::hash(&content).to_hex().to_string(),
        created_by: actor_id.clone(),
        created_at: now,
    })
}
