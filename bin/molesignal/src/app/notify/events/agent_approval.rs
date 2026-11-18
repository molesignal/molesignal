// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tells the people who can approve that an operation waits for them: when an approval request is
//! created pending, the event `agent.approval.requested` goes to the members holding
//! `agent.approve` (the requester left out, at most [`MOST_APPROVERS_NOTIFIED`]) through the
//! notify policies of that event. Sent to an IM App that hears presses, it carries approve and
//! reject buttons (see the notify engine's buttons).
//!
//! The work happens in the background after the request is saved: a failure is logged and never
//! fails the request. The text is in the workspace's language, from `i18n_language`.

use std::{collections::BTreeMap, sync::Arc};

use agent::model::ApprovalRequest;
use domain::{
    i18n_language::{FALLBACK_LANGUAGE_CODE, I18nLanguageRepository, LanguagePack},
    notify::{connector::NotifyMessage, policy::NotifyEvent},
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use postgres::repositories::workspace_preference_defaults::WorkspacePreferenceDefaultsRepository;
use serde_json::{Value, json};

use crate::app::{
    iam::{IamAccessService, IamService, member::member_context},
    notify::{NotifyDispatch, NotifyEngine},
};

pub const AGENT_APPROVAL_REQUESTED_EVENT: &str = "agent.approval.requested";

/// Every text of the notification is under this prefix.
pub const AGENT_APPROVAL_LANGUAGE_PREFIX: &str = "agent_approval.";

/// The most people one request is sent to.
pub const MOST_APPROVERS_NOTIFIED: usize = 20;

/// The most members looked at to find who can approve.
const MOST_MEMBERS_CHECKED: usize = 500;

/// The permission that reviews an approval request.
const APPROVE_PERMISSION: &str = "agent.approve";

/// Sends the notification of a new approval request.
#[derive(Clone)]
pub struct AgentApprovalNotice {
    engine: Arc<NotifyEngine>,
    language: Arc<dyn I18nLanguageRepository>,
    workspace: Arc<dyn WorkspacePreferenceDefaultsRepository>,
    iam: Arc<IamService>,
    access: Arc<IamAccessService>,
}

impl AgentApprovalNotice {
    pub fn new(
        engine: Arc<NotifyEngine>,
        language: Arc<dyn I18nLanguageRepository>,
        workspace: Arc<dyn WorkspacePreferenceDefaultsRepository>,
        iam: Arc<IamService>,
        access: Arc<IamAccessService>,
    ) -> Self {
        Self {
            engine,
            language,
            workspace,
            iam,
            access,
        }
    }

    /// Queues the notification of `approval` in the background.
    pub fn requested(&self, approval: &ApprovalRequest) {
        let notice = self.clone();
        let approval = approval.clone();

        tokio::spawn(async move {
            if let Err(error) = notice.send(&approval).await {
                tracing::warn!(
                    org_id = %approval.org_id,
                    approval_id = %approval.id,
                    %error,
                    "the approval request could not be notified"
                );
            }
        });
    }

    async fn send(&self, approval: &ApprovalRequest) -> Result<()> {
        let approvers = self
            .approvers(&approval.org_id, &approval.requested_by)
            .await?;

        if approvers.is_empty() {
            tracing::debug!(
                approval_id = %approval.id,
                "nobody else can approve the request; no notification"
            );

            return Ok(());
        }

        let language = self
            .workspace
            .get(&approval.org_id)
            .await
            .map(|defaults| defaults.language)
            .unwrap_or_else(|_| FALLBACK_LANGUAGE_CODE.to_string());

        let pack = self
            .language
            .load_pack(AGENT_APPROVAL_LANGUAGE_PREFIX, &language)
            .await?;

        let requester = self.name(&approval.requested_by).await;
        let url = approvals_url(self.engine.external_url());

        let dispatch = approval_dispatch(approval, &requester, &approvers, &pack, url.as_deref())?;

        self.engine.enqueue_event(dispatch).await?;
        Ok(())
    }

    /// The members of the organization who hold `agent.approve`, without the requester.
    async fn approvers(&self, org_id: &Id, requester: &Id) -> Result<Vec<Id>> {
        let members = self.iam.iam_memberships.list_for_org(org_id).await?;
        let mut candidates = Vec::new();

        for membership in members.into_iter().take(MOST_MEMBERS_CHECKED) {
            if &membership.user_id == requester {
                continue;
            }

            if candidates.iter().filter(|(_, holds)| *holds).count() >= MOST_APPROVERS_NOTIFIED {
                break;
            }

            let unusable = |_: &str, why: &str| Error::forbidden(why.to_string());

            let holds = match member_context(
                &self.iam,
                &self.access,
                org_id,
                &membership.user_id,
                unusable,
            )
            .await
            {
                Ok(member) => member.context.has_permission(APPROVE_PERMISSION),
                Err(Error::Forbidden(_) | Error::NotFound(_)) => false,
                Err(error) => return Err(error),
            };

            candidates.push((membership.user_id, holds));
        }

        Ok(pick_approvers(
            candidates,
            requester,
            MOST_APPROVERS_NOTIFIED,
        ))
    }

    /// A person's name, else their email, else their ID.
    async fn name(&self, user_id: &Id) -> String {
        match self.iam.current_user(user_id).await {
            Ok(user) if !user.display_name.trim().is_empty() => user.display_name,
            Ok(user) => user.email,
            Err(_) => user_id.to_string(),
        }
    }
}

/// Who is told: the candidates holding the permission, the requester never, each once, at most
/// `cap`, in the order given.
pub fn pick_approvers(candidates: Vec<(Id, bool)>, requester: &Id, cap: usize) -> Vec<Id> {
    let mut picked: Vec<Id> = Vec::new();

    for (user_id, holds) in candidates {
        if !holds || &user_id == requester || picked.contains(&user_id) {
            continue;
        }

        picked.push(user_id);

        if picked.len() >= cap {
            break;
        }
    }

    picked
}

/// The approvals page under MoleSignal's external address; none when no address is set.
pub fn approvals_url(external_url: &str) -> Option<String> {
    let base = external_url.trim().trim_end_matches('/');

    (!base.is_empty()).then(|| format!("{base}/agent/approvals"))
}

/// The notification of a new request, sent to `approvers`.
pub fn approval_dispatch(
    approval: &ApprovalRequest,
    requester: &str,
    approvers: &[Id],
    pack: &LanguagePack,
    url: Option<&str>,
) -> Result<NotifyDispatch> {
    let message = approval_message(pack, approval, requester, url)?;

    let attributes = json!({
        "user_ids": approvers.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "approval_id": approval.id.to_string(),
        "action": approval.action,
        "target": approval.target,
        "risk": risk_text(approval),
        "reason": approval.reason,
        "impact": approval.impact,
        "requested_by": approval.requested_by.to_string(),
        "required_approvals": approval.required_approvals,
        "expires_at": approval.expires_at.map(|at| at.0),
        "url": url,
    });

    Ok(NotifyDispatch {
        event: NotifyEvent {
            id: format!("{AGENT_APPROVAL_REQUESTED_EVENT}:{}", approval.id),
            event_type: AGENT_APPROVAL_REQUESTED_EVENT.into(),
            organization_id: approval.org_id.clone(),
            occurred_at: approval.created_at,
            attributes,
        },
        message,
    })
}

/// The title and text that ask for a review, written with `pack` (keys under
/// [`AGENT_APPROVAL_LANGUAGE_PREFIX`]); the IM card is rewritten from it after a press.
pub fn approval_message(
    pack: &LanguagePack,
    approval: &ApprovalRequest,
    requester: &str,
    url: Option<&str>,
) -> Result<NotifyMessage> {
    let action = action_text(approval);
    let risk = risk_text(approval);
    let count = approval.required_approvals.max(1).to_string();
    let expires = approval.expires_at.map(readable_time).unwrap_or_default();

    let fills = [
        ("action", action.as_str()),
        ("target", approval.target.as_str()),
        ("requester", requester),
        ("risk", risk.as_str()),
        ("count", count.as_str()),
        ("reason", approval.reason.as_str()),
        ("impact", approval.impact.as_str()),
        ("expires", expires.as_str()),
    ];

    let title = pack.render("agent_approval.requested.title", &fills)?;
    let mut text = pack.render("agent_approval.requested.text", &fills)?;

    if let Some(url) = url {
        text.push('\n');
        text.push_str(&pack.render("agent_approval.requested.open", &[("url", url)])?);
    }

    let mut metadata = BTreeMap::new();
    metadata.insert("approval_id".into(), approval.id.to_string());
    metadata.insert("event_type".into(), AGENT_APPROVAL_REQUESTED_EVENT.into());

    Ok(NotifyMessage {
        title: title.clone(),
        text: text.clone(),
        markdown: Some(format!("**{title}**\n\n{text}")),
        html: None,
        metadata,
        provider_templates: Default::default(),
        event: None,
        card: None,
    })
}

/// The operation as people read it: its key, with the remediation action's name when it is one.
fn action_text(approval: &ApprovalRequest) -> String {
    match approval.parameters.get("action").and_then(Value::as_str) {
        Some(name) if !name.trim().is_empty() => format!("{} ({})", approval.action, name.trim()),
        _ => approval.action.clone(),
    }
}

/// `L3` and the like.
fn risk_text(approval: &ApprovalRequest) -> String {
    serde_json::to_value(approval.risk)
        .ok()
        .and_then(|value| value.as_str().map(str::to_uppercase))
        .unwrap_or_default()
}

/// A moment as the notification writes it (UTC), such as 2026-11-11 09:30 UTC.
fn readable_time(at: TimestampMicros) -> String {
    use chrono::TimeZone as _;

    chrono::Utc
        .timestamp_micros(at.0)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use agent::model::ApprovalStatus;
    use tool_runtime::RiskLevel;

    use super::*;
    use crate::i18n_language_seed::seeded_pack;

    fn approval() -> ApprovalRequest {
        ApprovalRequest {
            id: Id::from_string("appr-1"),
            org_id: Id::from_string("org-1"),
            investigation_id: None,
            action: "run_remediation_action".into(),
            target: "inc-1".into(),
            parameters: json!({"actionId": "act-1", "action": "Restart checkout"}),
            reason: "Pool exhausted".into(),
            impact: "Kubernetes Restart Deployment shop/checkout".into(),
            risk: RiskLevel::L3,
            status: ApprovalStatus::Pending,
            requested_by: Id::from_string("u-asker"),
            required_approvals: 2,
            reviews: json!([]),
            expires_at: Some(TimestampMicros(1_762_853_400_000_000)),
            decided_at: None,
            created_at: TimestampMicros(1_762_849_800_000_000),
            updated_at: TimestampMicros(1_762_849_800_000_000),
        }
    }

    #[test]
    fn the_requester_and_those_without_the_permission_are_not_told() {
        let asker = Id::from_string("u-asker");
        let candidates = vec![
            (Id::from_string("u-1"), true),
            (asker.clone(), true),
            (Id::from_string("u-2"), false),
            (Id::from_string("u-3"), true),
            (Id::from_string("u-1"), true),
        ];

        assert_eq!(
            pick_approvers(candidates, &asker, 20),
            vec![Id::from_string("u-1"), Id::from_string("u-3")]
        );
    }

    #[test]
    fn at_most_the_cap_is_told() {
        let candidates = (0..30)
            .map(|index| (Id::from_string(format!("u-{index}")), true))
            .collect::<Vec<_>>();

        let picked = pick_approvers(candidates, &Id::from_string("u-asker"), 20);

        assert_eq!(picked.len(), 20);
        assert_eq!(picked[0], Id::from_string("u-0"));
    }

    #[test]
    fn the_event_names_the_approvers_and_the_request() {
        let pack = seeded_pack(AGENT_APPROVAL_LANGUAGE_PREFIX, "en-us");
        let approvers = vec![Id::from_string("u-1"), Id::from_string("u-3")];

        let dispatch = approval_dispatch(
            &approval(),
            "Kenji",
            &approvers,
            &pack,
            Some("https://ms.example.com/agent/approvals"),
        )
        .unwrap();

        assert_eq!(dispatch.event.event_type, AGENT_APPROVAL_REQUESTED_EVENT);
        assert_eq!(dispatch.event.id, "agent.approval.requested:appr-1");
        assert_eq!(dispatch.event.attributes["user_ids"], json!(["u-1", "u-3"]));
        assert_eq!(dispatch.event.attributes["approval_id"], "appr-1");
        assert_eq!(dispatch.event.attributes["risk"], "L3");
        assert_eq!(dispatch.message.metadata["approval_id"], "appr-1");
        assert!(
            dispatch.message.title.contains("Restart checkout"),
            "{}",
            dispatch.message.title
        );
        assert!(
            dispatch.message.text.contains("Kenji"),
            "{}",
            dispatch.message.text
        );
        assert!(
            dispatch
                .message
                .text
                .contains("https://ms.example.com/agent/approvals"),
            "{}",
            dispatch.message.text
        );
    }

    #[test]
    fn the_text_is_written_in_both_languages() {
        for language in ["en-us", "zh-cn"] {
            let pack = seeded_pack(AGENT_APPROVAL_LANGUAGE_PREFIX, language);
            let message = approval_message(&pack, &approval(), "Kenji", None).unwrap();

            assert!(!message.title.starts_with("agent_approval."), "{language}");
            assert!(message.text.contains("Pool exhausted"), "{language}");
            assert!(message.text.contains("2025-11-11 09:30 UTC"), "{language}");
        }
    }

    #[test]
    fn no_address_gives_no_link() {
        assert_eq!(approvals_url("  "), None);
        assert_eq!(
            approvals_url("https://ms.example.com/"),
            Some("https://ms.example.com/agent/approvals".into())
        );
    }
}
