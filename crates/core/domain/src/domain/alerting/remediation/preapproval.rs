// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A remediation action pre-approved. The organization turns pre-approval on (it is off until it
//! does), and people who may pre-approve say beforehand that the action may run on incidents of
//! some services, a few times within a stretch of time, until a date. A request the pre-approval
//! covers is approved by those people at once and runs without waiting; when it does not cover
//! one (another service, the runs used up, expired, turned off), the request waits for approval
//! as before.
//!
//! A pre-approval moves the approvals earlier and never lowers how many people approve: an L3
//! action needs a second person to confirm it before it takes effect. The person who starts the
//! action still needs the right to start remediation.

use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

use super::{RemediationAction, RemediationRisk, RemediationRun};
use crate::domain::{alerting::incident::Incident, notify::connector::NotifyTargetType};

pub const MAX_PREAPPROVAL_SERVICES: usize = 20;
pub const MAX_PREAPPROVAL_RUNS: u32 = 20;
pub const MIN_PREAPPROVAL_WINDOW_SECS: u32 = 300;
pub const MAX_PREAPPROVAL_WINDOW_SECS: u32 = 604_800;
pub const MAX_PREAPPROVAL_CHANNELS: usize = 5;

/// The longest group or address a channel names.
const MAX_TARGET_CHARS: usize = 500;

/// Where the notification after a pre-approved run goes: a connector and a group or an address
/// on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreapprovalChannel {
    pub connector_id: Id,
    pub target_type: NotifyTargetType,
    pub target: String,
}

/// A person who pre-approved, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreapprovalApprover {
    pub user_id: Id,
    pub at: TimestampMicros,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemediationPreapproval {
    /// The services whose incidents it covers.
    pub services: Vec<String>,
    /// At most this many runs within any `window_secs`.
    pub max_runs: u32,
    pub window_secs: u32,
    #[serde(default)]
    pub expires_at: Option<TimestampMicros>,
    /// Told after every pre-approved run, whether it worked or not.
    #[serde(default)]
    pub channels: Vec<PreapprovalChannel>,
    /// Who pre-approved: the person who set it first, then whoever confirmed it.
    #[serde(default)]
    pub approvers: Vec<PreapprovalApprover>,
    pub set_by: Id,
    pub set_at: TimestampMicros,
}

/// Where a pre-approval stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreapprovalState {
    /// The organization has pre-approval turned off.
    Off,
    Expired,
    /// It waits for the second person an L3 action needs.
    AwaitingApprover,
    /// As many runs as it allows went out within the window.
    UsedUp,
    Active,
}

/// The organization's switch, an `org_settings` section: off until someone turns it on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreapprovalSettings {
    #[serde(default)]
    pub enabled: bool,
}

impl RemediationPreapproval {
    /// How many people pre-approve an action of this risk: one for L2, two for L3.
    pub const fn approvers_needed(risk: RemediationRisk) -> usize {
        match risk {
            RemediationRisk::L2 => 1,
            RemediationRisk::L3 => 2,
        }
    }

    /// Trims what was set and checks it against the action it is for and `now`.
    pub fn validate(&mut self, action: &RemediationAction, now: TimestampMicros) -> Result<()> {
        let mut services: Vec<String> = Vec::new();

        for service in &self.services {
            let service = service.trim();

            if !service.is_empty()
                && !services
                    .iter()
                    .any(|seen| seen.eq_ignore_ascii_case(service))
            {
                services.push(service.to_string());
            }
        }

        if services.is_empty() {
            return Err(Error::invalid(
                "a pre-approval names the services whose incidents it covers",
            ));
        }

        if services.len() > MAX_PREAPPROVAL_SERVICES {
            return Err(Error::invalid(format!(
                "a pre-approval covers at most {MAX_PREAPPROVAL_SERVICES} services"
            )));
        }

        if let Some(outside) = services.iter().find(|service| {
            !action.services.is_empty()
                && !action
                    .services
                    .iter()
                    .any(|own| own.trim().eq_ignore_ascii_case(service))
        }) {
            return Err(Error::invalid(format!(
                "`{outside}` is not one of the services the action is for"
            )));
        }

        self.services = services;

        if !(1..=MAX_PREAPPROVAL_RUNS).contains(&self.max_runs) {
            return Err(Error::invalid(format!(
                "a pre-approval allows 1 to {MAX_PREAPPROVAL_RUNS} runs"
            )));
        }

        if !(MIN_PREAPPROVAL_WINDOW_SECS..=MAX_PREAPPROVAL_WINDOW_SECS).contains(&self.window_secs)
        {
            return Err(Error::invalid(format!(
                "the window is {MIN_PREAPPROVAL_WINDOW_SECS} to {MAX_PREAPPROVAL_WINDOW_SECS} seconds"
            )));
        }

        if self.expires_at.is_some_and(|expires| expires.0 <= now.0) {
            return Err(Error::invalid(
                "the pre-approval would already have expired",
            ));
        }

        if self.channels.len() > MAX_PREAPPROVAL_CHANNELS {
            return Err(Error::invalid(format!(
                "a pre-approval notifies at most {MAX_PREAPPROVAL_CHANNELS} channels"
            )));
        }

        for channel in &mut self.channels {
            channel.target = channel.target.trim().to_string();

            if channel.target.is_empty() || channel.target.chars().count() > MAX_TARGET_CHARS {
                return Err(Error::invalid(format!(
                    "a channel names a group or an address of 1 to {MAX_TARGET_CHARS} characters"
                )));
            }

            if !matches!(
                channel.target_type,
                NotifyTargetType::FixedAddress | NotifyTargetType::FixedGroup
            ) {
                return Err(Error::invalid(
                    "a channel is a fixed group or a fixed address",
                ));
            }
        }

        Ok(())
    }

    /// How many different people pre-approved.
    pub fn approver_count(&self) -> usize {
        let mut seen: Vec<&Id> = Vec::new();

        for approver in &self.approvers {
            if !seen.contains(&&approver.user_id) {
                seen.push(&approver.user_id);
            }
        }

        seen.len()
    }

    /// How many different people besides `requester` pre-approved. As in an ordinary approval,
    /// the person who asks for a run never approves it: their own pre-approval does not count.
    pub fn approvers_besides(&self, requester: &Id) -> usize {
        let mut seen: Vec<&Id> = Vec::new();

        for approver in &self.approvers {
            if &approver.user_id != requester && !seen.contains(&&approver.user_id) {
                seen.push(&approver.user_id);
            }
        }

        seen.len()
    }

    /// Whether it approves a run `requester` asks for: enough people besides them pre-approved.
    pub fn approves_for(&self, requester: &Id, risk: RemediationRisk) -> bool {
        self.approvers_besides(requester) >= Self::approvers_needed(risk)
    }

    pub fn has_approver(&self, user_id: &Id) -> bool {
        self.approvers
            .iter()
            .any(|approver| &approver.user_id == user_id)
    }

    /// Adds `user_id` as the next person who pre-approves. Refused when they already did or when
    /// no one more is needed.
    pub fn confirm(
        &mut self,
        user_id: &Id,
        risk: RemediationRisk,
        now: TimestampMicros,
    ) -> Result<()> {
        if self.has_approver(user_id) {
            return Err(Error::conflict(
                "the same person cannot pre-approve twice; another person confirms it",
            ));
        }

        if self.approver_count() >= Self::approvers_needed(risk) {
            return Err(Error::conflict(
                "the pre-approval already has the approvers it needs",
            ));
        }

        self.approvers.push(PreapprovalApprover {
            user_id: user_id.clone(),
            at: now,
        });

        Ok(())
    }

    /// Where it stands at `now`, with the organization's switch `enabled` and `used` runs within
    /// the window.
    pub fn state(
        &self,
        enabled: bool,
        risk: RemediationRisk,
        used: u32,
        now: TimestampMicros,
    ) -> PreapprovalState {
        if !enabled {
            PreapprovalState::Off
        } else if self.expires_at.is_some_and(|expires| expires.0 <= now.0) {
            PreapprovalState::Expired
        } else if self.approver_count() < Self::approvers_needed(risk) {
            PreapprovalState::AwaitingApprover
        } else if used >= self.max_runs {
            PreapprovalState::UsedUp
        } else {
            PreapprovalState::Active
        }
    }

    /// Whether the incident is one of its services'.
    pub fn covers(&self, incident: &Incident) -> bool {
        self.services.iter().any(|service| {
            incident
                .affected_services
                .iter()
                .chain(incident.labels.get("service"))
                .any(|own| own.trim().eq_ignore_ascii_case(service.trim()))
        })
    }

    /// Where the stretch whose runs count begins.
    pub fn window_start(&self, now: TimestampMicros) -> TimestampMicros {
        TimestampMicros(
            now.0
                .saturating_sub(i64::from(self.window_secs) * 1_000_000),
        )
    }
}

/// Whether an edit changes what the people who pre-approved said yes to: where and how it runs,
/// what it asks for, which incidents it is for, or its risk. Their approvals do not carry over.
pub fn changes_what_was_approved(before: &RemediationAction, after: &RemediationAction) -> bool {
    before.target != after.target
        || before.parameters != after.parameters
        || before.services != after.services
        || before.team_ids != after.team_ids
        || before.risk != after.risk
}

/// Where pre-approvals and the organization's switch are kept, beside the actions and their runs.
#[async_trait]
pub trait PreapprovalRepository: Send + Sync {
    /// Writes the action's pre-approval; `None` takes it away. `false` when there is no such
    /// action.
    async fn save_preapproval(
        &self,
        org_id: &Id,
        action_id: &Id,
        preapproval: Option<&RemediationPreapproval>,
    ) -> Result<bool>;

    /// How many runs of the action went out under its pre-approval since `since`.
    async fn preapproved_runs_since(
        &self,
        org_id: &Id,
        action_id: &Id,
        since: TimestampMicros,
    ) -> Result<u32>;

    /// Records `run` as pre-approved when fewer than `max_runs` runs of its action went out under
    /// the pre-approval since `since`; `None` when they are used up. Two requests at the same
    /// time never both take the last one.
    async fn claim_preapproved_run(
        &self,
        run: RemediationRun,
        max_runs: u32,
        since: TimestampMicros,
    ) -> Result<Option<RemediationRun>>;

    async fn preapproval_settings(&self, org_id: &Id) -> Result<PreapprovalSettings>;

    async fn save_preapproval_settings(
        &self,
        org_id: &Id,
        settings: PreapprovalSettings,
        updated_by: &Id,
        now: TimestampMicros,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::alerting::remediation::{
        HttpRemediation, HttpVerb, RemediationTarget, VerifyBy,
    };

    const NOW: TimestampMicros = TimestampMicros(1_762_849_800_000_000);
    const HOUR: i64 = 3_600_000_000;

    fn action(risk: RemediationRisk) -> RemediationAction {
        RemediationAction {
            id: Id::from_string("act-1"),
            org_id: Id::from_string("org-1"),
            name: "Restart checkout".into(),
            description: String::new(),
            target: RemediationTarget::Http(HttpRemediation {
                method: HttpVerb::Post,
                url: "https://ci.example.com/restart".into(),
                headers: BTreeMap::new(),
                body: String::new(),
                signing_secret: None,
            }),
            parameters: Vec::new(),
            services: vec!["checkout".into(), "payments".into()],
            team_ids: Vec::new(),
            risk,
            verify_after_secs: 0,
            verify_by: VerifyBy::IncidentResolved,
            enabled: true,
            secrets_set: Vec::new(),
            preapproval: None,
            created_by: None,
            updated_by: None,
            created_at: NOW,
            updated_at: NOW,
        }
    }

    fn preapproval() -> RemediationPreapproval {
        RemediationPreapproval {
            services: vec![" Checkout ".into(), "checkout".into()],
            max_runs: 2,
            window_secs: 3_600,
            expires_at: Some(TimestampMicros(NOW.0 + 24 * HOUR)),
            channels: vec![PreapprovalChannel {
                connector_id: Id::from_string("feishu-1"),
                target_type: NotifyTargetType::FixedGroup,
                target: " oc_ops ".into(),
            }],
            approvers: vec![PreapprovalApprover {
                user_id: Id::from_string("u-1"),
                at: NOW,
            }],
            set_by: Id::from_string("u-1"),
            set_at: NOW,
        }
    }

    fn incident(services: &[&str]) -> Incident {
        Incident {
            id: Id::from_string("inc-1"),
            org_id: Id::from_string("org-1"),
            rule_id: Id::from_string("rule-1"),
            escalation_policy_id: Id::from_string("default"),
            status: crate::domain::alerting::incident::IncidentStatus::Open,
            severity: crate::domain::alerting::incident::Severity::Critical,
            summary: "Checkout errors".into(),
            fingerprint: "fp".into(),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: NOW,
            assignees: Vec::new(),
            labels: BTreeMap::new(),
            annotations: BTreeMap::new(),
            trace_ids: Vec::new(),
            host_ids: Vec::new(),
            affected_services: services
                .iter()
                .map(|service| (*service).to_string())
                .collect(),
            triggering_query: None,
            created_at: NOW,
            acknowledged_at: None,
            acknowledged_by: None,
            resolved_at: None,
            resolved_by: None,
            team_id: None,
            routing: None,
            handling: Default::default(),
            revision: 0,
        }
    }

    #[test]
    fn it_covers_only_the_action_s_own_services() {
        let mut set = preapproval();
        set.validate(&action(RemediationRisk::L2), NOW).unwrap();

        assert_eq!(set.services, vec!["Checkout".to_string()]);
        assert_eq!(set.channels[0].target, "oc_ops");

        let mut outside = preapproval();
        outside.services = vec!["search".into()];
        assert!(outside.validate(&action(RemediationRisk::L2), NOW).is_err());

        let mut none = preapproval();
        none.services = vec!["  ".into()];
        assert!(none.validate(&action(RemediationRisk::L2), NOW).is_err());
    }

    #[test]
    fn runs_window_and_expiry_are_bounded() {
        let action = action(RemediationRisk::L2);

        let mut many = preapproval();
        many.max_runs = 21;
        assert!(many.validate(&action, NOW).is_err());

        let mut short = preapproval();
        short.window_secs = 60;
        assert!(short.validate(&action, NOW).is_err());

        let mut past = preapproval();
        past.expires_at = Some(TimestampMicros(NOW.0 - 1));
        assert!(past.validate(&action, NOW).is_err());

        let mut personal = preapproval();
        personal.channels[0].target_type = NotifyTargetType::DirectUser;
        assert!(personal.validate(&action, NOW).is_err());
    }

    #[test]
    fn an_l3_action_waits_for_a_second_person() {
        let mut set = preapproval();

        assert_eq!(
            set.state(true, RemediationRisk::L3, 0, NOW),
            PreapprovalState::AwaitingApprover
        );
        assert_eq!(
            set.state(true, RemediationRisk::L2, 0, NOW),
            PreapprovalState::Active
        );

        assert!(
            set.confirm(&Id::from_string("u-1"), RemediationRisk::L3, NOW)
                .is_err()
        );

        set.confirm(&Id::from_string("u-2"), RemediationRisk::L3, NOW)
            .unwrap();

        assert_eq!(set.approver_count(), 2);
        assert_eq!(
            set.state(true, RemediationRisk::L3, 0, NOW),
            PreapprovalState::Active
        );
        assert!(
            set.confirm(&Id::from_string("u-3"), RemediationRisk::L3, NOW)
                .is_err()
        );
    }

    #[test]
    fn a_requester_s_own_pre_approval_does_not_approve_their_request() {
        let mut set = preapproval();
        let setter = Id::from_string("u-1");
        let other = Id::from_string("u-7");

        assert!(set.approves_for(&other, RemediationRisk::L2));
        assert!(!set.approves_for(&setter, RemediationRisk::L2));

        set.confirm(&Id::from_string("u-2"), RemediationRisk::L3, NOW)
            .unwrap();

        assert!(set.approves_for(&other, RemediationRisk::L3));
        assert_eq!(set.approvers_besides(&setter), 1);
        assert!(!set.approves_for(&setter, RemediationRisk::L3));
        assert!(set.approves_for(&setter, RemediationRisk::L2));
    }

    #[test]
    fn it_stops_when_turned_off_expired_or_used_up() {
        let set = preapproval();

        assert_eq!(
            set.state(false, RemediationRisk::L2, 0, NOW),
            PreapprovalState::Off
        );
        assert_eq!(
            set.state(true, RemediationRisk::L2, 2, NOW),
            PreapprovalState::UsedUp
        );
        assert_eq!(
            set.state(
                true,
                RemediationRisk::L2,
                0,
                TimestampMicros(NOW.0 + 25 * HOUR)
            ),
            PreapprovalState::Expired
        );
        assert_eq!(set.window_start(NOW), TimestampMicros(NOW.0 - HOUR));
    }

    #[test]
    fn it_covers_an_incident_of_one_of_its_services() {
        let mut set = preapproval();
        set.validate(&action(RemediationRisk::L2), NOW).unwrap();

        assert!(set.covers(&incident(&["checkout"])));
        assert!(!set.covers(&incident(&["payments"])));

        let mut labelled = incident(&[]);
        labelled.labels.insert("service".into(), "CHECKOUT".into());
        assert!(set.covers(&labelled));
    }

    #[test]
    fn an_edit_of_how_it_runs_takes_the_approvals_away() {
        let before = action(RemediationRisk::L2);
        let mut renamed = before.clone();
        renamed.name = "Restart checkout pods".into();
        assert!(!changes_what_was_approved(&before, &renamed));

        let mut riskier = before.clone();
        riskier.risk = RemediationRisk::L3;
        assert!(changes_what_was_approved(&before, &riskier));

        let mut wider = before.clone();
        wider.services.push("search".into());
        assert!(changes_what_was_approved(&before, &wider));
    }
}
