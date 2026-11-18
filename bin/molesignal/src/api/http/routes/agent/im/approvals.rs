// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The approve and reject buttons on an approval request sent to IM ([`AlertAction::Approve`],
//! [`AlertAction::Reject`]). The person who pressed is matched the way alert buttons match them
//! (verified notification identities only; `alerts::settle` offers the link or refuses an account
//! linked to several people before this is reached), and reviews as themselves with what they
//! hold in MoleSignal: `agent.approve`, and never their own request. A request that reaches the
//! approvals it needs runs at once, as it does from the approvals page. The card is rewritten to
//! say how it ended, without the buttons.

use agent::{
    im::{AlertCard, CardAction, CardAnswer, NoticeKind},
    model::{ApprovalRequest, ApprovalStatus, ExecutionStatus},
};
use domain::notify::{
    buttons::{AlertAction, AlertButtons},
    connector::NotifyMessage,
};
use kernel::{Error, Result, ids::Id};

use super::{inbound::principal, texts::Texts};
use crate::{
    api::{
        AppState,
        http::routes::agent::{
            chat::run::identity::resolve_principal,
            control::review::{self, Reviewed},
        },
    },
    app::notify::{AGENT_APPROVAL_LANGUAGE_PREFIX, approval_message, approvals_url},
};

/// How a press ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Ending {
    /// Approved; the request waits for this many more approvals.
    Waiting(i32),
    Ran,
    RunFailed(String),
    /// Approved, but the operation could not be started.
    NotStarted(String),
    Rejected,
    Expired,
    /// Somebody decided it before this press; how it stands now.
    AlreadyDecided(ApprovalStatus),
    /// This person reviewed it before; it still waits for others.
    AlreadyReviewed,
}

/// The pressed button decides the approval request `action.value.alert_id` as `user_id`.
pub(super) async fn carry_out(
    state: &AppState,
    org_id: &Id,
    user_id: &Id,
    action: &CardAction,
) -> Result<CardAnswer> {
    let texts = Texts::for_user(state, org_id, Some(user_id)).await;

    let ctx = match resolve_principal(state, org_id, &principal(user_id)).await {
        Ok(ctx) => ctx,
        Err(Error::Forbidden(_) | Error::NotFound(_)) => {
            return Ok(notice(
                NoticeKind::Warning,
                texts.get("approval.no_access", &[]),
            ));
        }
        Err(error) => return Err(error),
    };

    if !ctx.has_permission("agent.approve") {
        return Ok(notice(
            NoticeKind::Warning,
            texts.get("approval.no_access", &[]),
        ));
    }

    let id = Id::from_string(action.value.alert_id.clone());
    let approve = action.value.action == AlertAction::Approve;

    let (ending, approval) = match review::review(state, &ctx, &id, approve, "").await {
        Ok(reviewed) => (ending_of(&reviewed, approve), reviewed.approval),
        Err(Error::NotFound(_)) => {
            return Ok(notice(
                NoticeKind::Warning,
                texts.get("approval.not_found", &[]),
            ));
        }
        // The only refusal before the review: the requester approving their own request.
        Err(Error::Forbidden(_)) => {
            return Ok(notice(NoticeKind::Warning, texts.get("approval.own", &[])));
        }
        // Decided already, or this person reviewed it already: say how it stands.
        Err(Error::Conflict(_)) => {
            let approval = state.agent.repository.get_approval(org_id, &id).await?;

            let ending = if approval.status == ApprovalStatus::Pending {
                Ending::AlreadyReviewed
            } else {
                Ending::AlreadyDecided(approval.status)
            };

            (ending, approval)
        }
        Err(error) => return Err(error),
    };

    let user = name(state, user_id).await;
    let (kind, said, note) = words(&ending, &texts, &user);

    Ok(CardAnswer {
        notice: Some((kind, said)),
        card: Some(card(state, &texts, &approval, action, note).await),
    })
}

fn notice(kind: NoticeKind, text: String) -> CardAnswer {
    CardAnswer {
        notice: Some((kind, text)),
        card: None,
    }
}

/// How the review ended, from what it did.
fn ending_of(reviewed: &Reviewed, approve: bool) -> Ending {
    let ran = reviewed
        .execution
        .as_ref()
        .map(|execution| match execution {
            Ok(completed) => Ok((
                completed.execution.status,
                completed.execution.error.clone(),
            )),
            Err(error) => Err(error.to_string()),
        });

    let approved = reviewed
        .approval
        .reviews
        .as_array()
        .into_iter()
        .flatten()
        .filter(|review| review["decision"] == "approved")
        .count();

    let waiting = reviewed
        .approval
        .required_approvals
        .saturating_sub(i32::try_from(approved).unwrap_or(i32::MAX))
        .max(1);

    ending(reviewed.approval.status, approve, waiting, ran)
}

/// How a review ended: the request's status after it, whether it approved, how many approvals it
/// still waits for, and how its run went when it ran.
pub(super) fn ending(
    status: ApprovalStatus,
    approve: bool,
    waiting: i32,
    ran: Option<std::result::Result<(ExecutionStatus, Option<String>), String>>,
) -> Ending {
    match (ran, status) {
        (Some(Ok((ExecutionStatus::Succeeded | ExecutionStatus::PartiallySucceeded, _))), _) => {
            Ending::Ran
        }
        (Some(Ok((_, error))), _) => Ending::RunFailed(error.unwrap_or_default()),
        (Some(Err(error)), _) => Ending::NotStarted(error),
        (None, ApprovalStatus::Expired) => Ending::Expired,
        (None, ApprovalStatus::Rejected) if !approve => Ending::Rejected,
        (None, ApprovalStatus::Pending) if approve => Ending::Waiting(waiting),
        (None, status) => Ending::AlreadyDecided(status),
    }
}

/// The keys (under `agent.im.`) of what the presser is told and of the line on the card.
pub(super) fn keys(ending: &Ending) -> (NoticeKind, &'static str, Option<&'static str>) {
    match ending {
        Ending::Waiting(_) => (
            NoticeKind::Success,
            "approval.done_waiting",
            Some("approval.waiting_by"),
        ),
        Ending::Ran => (
            NoticeKind::Success,
            "approval.done_ran",
            Some("approval.ran_by"),
        ),
        Ending::RunFailed(_) => (
            NoticeKind::Warning,
            "approval.done_run_failed",
            Some("approval.run_failed_by"),
        ),
        Ending::NotStarted(_) => (
            NoticeKind::Warning,
            "approval.done_not_started",
            Some("approval.not_started_by"),
        ),
        Ending::Rejected => (
            NoticeKind::Success,
            "approval.done_rejected",
            Some("approval.rejected_by"),
        ),
        Ending::Expired => (
            NoticeKind::Info,
            "approval.expired",
            Some("approval.note_expired"),
        ),
        Ending::AlreadyDecided(status) => match status {
            ApprovalStatus::Approved => (
                NoticeKind::Info,
                "approval.already_approved",
                Some("approval.note_approved"),
            ),
            ApprovalStatus::Executed => (
                NoticeKind::Info,
                "approval.already_executed",
                Some("approval.note_executed"),
            ),
            ApprovalStatus::Rejected => (
                NoticeKind::Info,
                "approval.already_rejected",
                Some("approval.note_rejected"),
            ),
            ApprovalStatus::Expired => (
                NoticeKind::Info,
                "approval.expired",
                Some("approval.note_expired"),
            ),
            ApprovalStatus::Cancelled => (
                NoticeKind::Info,
                "approval.cancelled",
                Some("approval.note_cancelled"),
            ),
            ApprovalStatus::Pending => (NoticeKind::Info, "approval.already_reviewed", None),
        },
        Ending::AlreadyReviewed => (NoticeKind::Info, "approval.already_reviewed", None),
    }
}

/// What the presser is told and the line on the card, filled in.
fn words(ending: &Ending, texts: &Texts, user: &str) -> (NoticeKind, String, Option<String>) {
    let (count, reason) = match ending {
        Ending::Waiting(count) => (count.to_string(), String::new()),
        Ending::RunFailed(reason) | Ending::NotStarted(reason) => (String::new(), reason.clone()),
        _ => (String::new(), String::new()),
    };

    let fills = [
        ("user", user),
        ("count", count.as_str()),
        ("reason", reason.as_str()),
    ];

    let (kind, said, note) = keys(ending);

    (
        kind,
        texts.get(said, &fills),
        note.map(|note| texts.get(note, &fills)),
    )
}

/// The card as it now is: the request's text in the presser's language, the line saying how it
/// ended, and "Open in MoleSignal" instead of the buttons.
async fn card(
    state: &AppState,
    texts: &Texts,
    approval: &ApprovalRequest,
    action: &CardAction,
    note: Option<String>,
) -> AlertCard {
    let pack = state
        .platform
        .i18n_language
        .load_pack(AGENT_APPROVAL_LANGUAGE_PREFIX, &texts.language)
        .await
        .unwrap_or_default();

    let requester = name(state, &approval.requested_by).await;
    let url = approvals_url(&state.platform.external_url);

    let message =
        approval_message(&pack, approval, &requester, url.as_deref()).unwrap_or_else(|_| {
            NotifyMessage {
                title: approval.action.clone(),
                text: approval.reason.clone(),
                markdown: None,
                html: None,
                metadata: Default::default(),
                provider_templates: Default::default(),
                event: None,
                card: None,
            }
        });

    let buttons = AlertButtons {
        alert_id: approval.id.to_string(),
        event_type: action.value.event_type.clone(),
        policy_id: action.value.policy_id.clone(),
        alert_ids: Vec::new(),
        actions: Vec::new(),
        open: url.map(|url| (texts.get("open", &[]), url)),
        callback: None,
    };

    AlertCard {
        message,
        buttons,
        note,
    }
}

/// A user's name in MoleSignal; their email when they have none.
async fn name(state: &AppState, user_id: &Id) -> String {
    match state.iam.service.users.get(user_id).await {
        Ok(user) if !user.display_name.trim().is_empty() => user.display_name,
        Ok(user) => user.email,
        Err(_) => user_id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_ends_as_the_review_and_the_run_went() {
        assert_eq!(
            ending(ApprovalStatus::Pending, true, 1, None),
            Ending::Waiting(1)
        );
        assert_eq!(
            ending(
                ApprovalStatus::Executed,
                true,
                1,
                Some(Ok((ExecutionStatus::Succeeded, None)))
            ),
            Ending::Ran
        );
        assert_eq!(
            ending(
                ApprovalStatus::Executed,
                true,
                1,
                Some(Ok((ExecutionStatus::Failed, Some("HTTP 500".into()))))
            ),
            Ending::RunFailed("HTTP 500".into())
        );
        assert_eq!(
            ending(
                ApprovalStatus::Approved,
                true,
                1,
                Some(Err("needs 2 approvals".into()))
            ),
            Ending::NotStarted("needs 2 approvals".into())
        );
        assert_eq!(
            ending(ApprovalStatus::Rejected, false, 1, None),
            Ending::Rejected
        );
        assert_eq!(
            ending(ApprovalStatus::Expired, true, 1, None),
            Ending::Expired
        );
    }

    #[test]
    fn the_card_says_who_did_what_and_a_late_press_how_it_stands() {
        assert_eq!(
            keys(&Ending::Waiting(1)),
            (
                NoticeKind::Success,
                "approval.done_waiting",
                Some("approval.waiting_by")
            )
        );
        assert_eq!(
            keys(&Ending::Rejected),
            (
                NoticeKind::Success,
                "approval.done_rejected",
                Some("approval.rejected_by")
            )
        );
        assert_eq!(
            keys(&Ending::AlreadyDecided(ApprovalStatus::Executed)),
            (
                NoticeKind::Info,
                "approval.already_executed",
                Some("approval.note_executed")
            )
        );
        assert_eq!(
            keys(&Ending::AlreadyReviewed),
            (NoticeKind::Info, "approval.already_reviewed", None)
        );
        assert_eq!(keys(&Ending::RunFailed("x".into())).0, NoticeKind::Warning);
    }

    #[test]
    fn the_words_are_filled_in() {
        let texts = Texts::empty();
        let (_, said, note) = words(&Ending::Waiting(1), &texts, "Alice");

        assert_eq!(said, "agent.im.approval.done_waiting");
        assert_eq!(note.as_deref(), Some("agent.im.approval.waiting_by"));
    }
}
