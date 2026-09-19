// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::{
    domain::status_page::{
        AutomationAction, AutomationMatchers, AutomationRuleLifecycle, IncidentImpact,
    },
    shared::{Error, Result, ids::Id},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationRuleInput {
    pub name: String,
    pub position: Option<i32>,
    pub matchers: AutomationMatchers,
    pub action: AutomationAction,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationSimulation {
    pub matched_rule_id: Option<crate::shared::ids::Id>,
    pub matched_revision_id: Option<crate::shared::ids::Id>,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AutomationLifecycleInput {
    pub lifecycle: AutomationRuleLifecycle,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AutomationRuleOrderInput {
    pub rule_ids: Vec<Id>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AutomationApprovalInput {
    pub title: String,
    pub impact: IncidentImpact,
    pub message: String,
    pub component_ids: Vec<Id>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AutomationSettingsInput {
    pub paused: bool,
}

pub(super) fn validate_decision_note(note: Option<&str>) -> Result<()> {
    if note.is_some_and(|note| note.chars().count() > 500) {
        return Err(Error::invalid(
            "automation Candidate decision note cannot exceed 500 characters",
        ));
    }
    Ok(())
}
