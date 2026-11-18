// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Applying a pre-approval when a remediation action is requested on an incident (see
//! [`domain::alerting::remediation::preapproval`]). The request is approved at once, in the name
//! of the people who pre-approved, when the organization has pre-approval on and the action's
//! pre-approval is in force, covers the incident's services and has a run left in its window; the
//! run is claimed here, and the caller records the approval already approved and runs it. Anything
//! else leaves the request to wait for approval as before.

use std::collections::BTreeMap;

use domain::alerting::{
    remediation::{
        PreapprovalState, RemediationPreapproval, RemediationRisk, RemediationRun,
        RemediationRunStatus,
    },
    timeline::TimelineKind,
};
use kernel::{Result, ids::Id, time::TimestampMicros};
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::RiskLevel;

use super::RemediationService;

/// Marks a review that a pre-approval gave.
pub const PREAPPROVAL_VIA: &str = "preapproval";

/// What a pre-approval gives one request.
#[derive(Debug, Clone, PartialEq)]
pub struct PreapprovalGrant {
    /// The reviews the approval request records: one per person who pre-approved.
    pub reviews: Value,
    /// As many approvals as the action's risk needs.
    pub required_approvals: i32,
    /// The action's own risk.
    pub risk: RiskLevel,
    pub run_id: Id,
}

/// The parameters of a `run_remediation_action` request.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Params {
    action_id: Id,
    #[serde(default)]
    inputs: BTreeMap<String, String>,
}

impl RemediationService {
    /// The grant of the action's pre-approval to a request on the incident `incident_id`, with the
    /// run claimed; `None` when no pre-approval covers it now.
    pub async fn preapproval_grant(
        &self,
        org_id: &Id,
        incident_id: &Id,
        parameters: &Value,
        approval_id: &Id,
        requested_by: &Id,
        now: TimestampMicros,
    ) -> Result<Option<PreapprovalGrant>> {
        let Ok(params) = serde_json::from_value::<Params>(parameters.clone()) else {
            return Ok(None);
        };

        let Ok(incident) = self.incidents.get(incident_id).await else {
            return Ok(None);
        };

        if &incident.org_id != org_id {
            return Ok(None);
        }

        // An action that is off, not for this incident or given wrong inputs waits as before:
        // running it fails there with the reason.
        let Ok((action, inputs)) = self
            .prepare(&incident, &params.action_id, &params.inputs)
            .await
        else {
            return Ok(None);
        };

        let Some(preapproval) = action.preapproval.clone() else {
            return Ok(None);
        };

        let enabled = self.repository.preapproval_settings(org_id).await?.enabled;

        // The runs left are counted when the run is claimed.
        // The requester's own pre-approval does not count, as in an ordinary approval: without
        // enough other people the request waits for approval as before.
        if preapproval.state(enabled, action.risk, 0, now) != PreapprovalState::Active
            || !preapproval.covers(&incident)
            || !preapproval.approves_for(requested_by, action.risk)
        {
            return Ok(None);
        }

        let run = RemediationRun {
            id: Id::new(),
            org_id: org_id.clone(),
            incident_id: incident.id.clone(),
            action_id: action.id.clone(),
            action_name: action.name.clone(),
            approval_id: approval_id.clone(),
            inputs,
            status: RemediationRunStatus::AwaitingApproval,
            output: String::new(),
            verify_at: None,
            verification: String::new(),
            requested_by: requested_by.clone(),
            executed_by: None,
            preapproved: true,
            created_at: now,
            updated_at: now,
        };

        let Some(run) = self
            .repository
            .claim_preapproved_run(run, preapproval.max_runs, preapproval.window_start(now))
            .await?
        else {
            return Ok(None);
        };

        let approvers = preapproval
            .approvers
            .iter()
            .filter(|approver| &approver.user_id != requested_by)
            .map(|approver| approver.user_id.clone())
            .collect::<Vec<_>>();

        self.record(
            &run,
            TimelineKind::RemediationRequested,
            Some(requested_by.clone()),
            json!({
                "runId": run.id,
                "action": run.action_name,
                "inputs": run.inputs,
                "preapproved": true,
                "approvers": approvers,
            }),
        )
        .await;

        Ok(Some(PreapprovalGrant {
            reviews: preapproval_reviews(&preapproval, requested_by),
            required_approvals: needed(action.risk),
            risk: match action.risk {
                RemediationRisk::L2 => RiskLevel::L2,
                RemediationRisk::L3 => RiskLevel::L3,
            },
            run_id: run.id,
        }))
    }

    /// The approval of a claimed run could not be recorded: the run does not happen.
    pub async fn release_preapproved_run(&self, org_id: &Id, approval_id: &Id) {
        let released = match self
            .repository
            .find_run_by_approval(org_id, approval_id)
            .await
        {
            Ok(Some(run)) => self
                .decline(run, "the approval could not be recorded")
                .await
                .map(|_| ()),
            Ok(None) => Ok(()),
            Err(error) => Err(error),
        };

        if let Err(error) = released {
            tracing::warn!(%approval_id, %error, "a pre-approved run could not be released");
        }
    }
}

/// How many approvals an action of this risk needs.
fn needed(risk: RemediationRisk) -> i32 {
    i32::try_from(RemediationPreapproval::approvers_needed(risk)).unwrap_or(2)
}

/// The reviews an approval request records for the people who pre-approved, in the shape of the
/// approval reviews (`reviewerId`, `decision`, `comment`, `reviewedAt`) with `via` saying where
/// they came from. The requester is left out: nobody approves their own request.
pub fn preapproval_reviews(preapproval: &RemediationPreapproval, requester: &Id) -> Value {
    let mut reviewers: Vec<&Id> = Vec::new();
    let mut reviews = Vec::new();

    for approver in &preapproval.approvers {
        if &approver.user_id == requester || reviewers.contains(&&approver.user_id) {
            continue;
        }

        reviewers.push(&approver.user_id);

        reviews.push(json!({
            "reviewerId": approver.user_id.0,
            "decision": "approved",
            "comment": "pre-approved",
            "reviewedAt": approver.at.0,
            "via": PREAPPROVAL_VIA,
        }));
    }

    Value::Array(reviews)
}

/// Whether an approval request's reviews came from a pre-approval.
pub fn is_preapproved(reviews: &Value) -> bool {
    reviews.as_array().is_some_and(|reviews| {
        !reviews.is_empty()
            && reviews
                .iter()
                .all(|review| review["via"].as_str() == Some(PREAPPROVAL_VIA))
    })
}

#[cfg(test)]
mod tests {
    use domain::alerting::remediation::preapproval::PreapprovalApprover;

    use super::*;

    fn preapproval(approvers: &[&str]) -> RemediationPreapproval {
        RemediationPreapproval {
            services: vec!["checkout".into()],
            max_runs: 2,
            window_secs: 3_600,
            expires_at: None,
            channels: Vec::new(),
            approvers: approvers
                .iter()
                .enumerate()
                .map(|(index, user)| PreapprovalApprover {
                    user_id: Id::from_string(*user),
                    at: TimestampMicros(1_000 + index as i64),
                })
                .collect(),
            set_by: Id::from_string("u-1"),
            set_at: TimestampMicros(1_000),
        }
    }

    #[test]
    fn every_person_who_pre_approved_is_one_approved_review() {
        let reviews = preapproval_reviews(
            &preapproval(&["u-1", "u-2", "u-1"]),
            &Id::from_string("u-9"),
        );

        assert_eq!(reviews.as_array().map(Vec::len), Some(2));
        assert_eq!(reviews[0]["reviewerId"], "u-1");
        assert_eq!(reviews[1]["reviewerId"], "u-2");
        assert_eq!(reviews[1]["decision"], "approved");
        assert!(is_preapproved(&reviews));
    }

    #[test]
    fn the_requester_is_never_one_of_the_reviews() {
        let reviews = preapproval_reviews(&preapproval(&["u-1", "u-2"]), &Id::from_string("u-1"));

        assert_eq!(reviews.as_array().map(Vec::len), Some(1));
        assert_eq!(reviews[0]["reviewerId"], "u-2");
    }

    #[test]
    fn reviews_people_gave_are_not_a_pre_approval() {
        assert!(!is_preapproved(&json!([])));
        assert!(!is_preapproved(&json!([
            {"reviewerId": "u-1", "decision": "approved"}
        ])));
        assert!(!is_preapproved(&Value::Null));
    }

    #[test]
    fn the_action_s_risk_sets_how_many_approve() {
        assert_eq!(needed(RemediationRisk::L2), 1);
        assert_eq!(needed(RemediationRisk::L3), 2);
    }
}
