// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A remediation action that a pre-approval covered: the request comes back already approved (see
//! `app::alerting::remediation::preapproval`) and runs at once, as the person who started it, from
//! Mole Agent's tools, inbound MCP and the incident page alike. Afterwards the audit log says it
//! ran under a pre-approval, and the pre-approval's channels hear how it went, whether it worked
//! or not, in the workspace's language (`remediation.preapproval.executed.*`).

use agent::model::ExecutionStatus;
use domain::{
    i18n_language::{FALLBACK_LANGUAGE_CODE, LanguagePack},
    iam::IamContext,
    notify::connector::{NotifyMessage, NotifyTarget},
};
use kernel::{Result, ids::Id};
use serde_json::{Value, json};
use tool_runtime::{ToolContent, ToolResult};

use super::{OperationExecution, execute_approved_operation};
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{alerting::remediation::preapproval::is_preapproved, notify::DirectSend},
};

/// Every text of the notification is under this prefix.
const LANGUAGE_KEY_PREFIX: &str = "remediation.preapproval.";

/// The approval request in a tool's result when a pre-approval approved it.
pub(crate) fn preapproved_in(result: &ToolResult) -> Option<Id> {
    if result.is_error {
        return None;
    }

    result
        .content
        .iter()
        .find_map(|content| match content {
            ToolContent::Json { json } => json.get("approval"),
            ToolContent::Text { .. } => None,
        })
        .filter(|approval| {
            approval["status"].as_str() == Some("approved") && is_preapproved(&approval["reviews"])
        })
        .and_then(|approval| approval["id"].as_str())
        .map(Id::from_string)
}

/// Runs the pre-approved request `approval_id` as `ctx`, then records and tells it.
pub(crate) async fn run(
    state: &AppState,
    ctx: &IamContext,
    approval_id: &Id,
) -> Result<OperationExecution> {
    let completed = execute_approved_operation(
        state,
        ctx,
        approval_id,
        format!("preapproval:{}", approval_id.0),
    )
    .await?;

    if let Err(error) = tell(state, ctx, approval_id, &completed).await {
        tracing::warn!(%approval_id, %error, "a pre-approved run could not be told");
    }

    Ok(completed)
}

/// What Mole Agent is given back: the run, and that it did not wait.
pub(crate) fn preapproved_result(approval_id: &Id, completed: OperationExecution) -> ToolResult {
    ToolResult::json(json!({
        "operationId": completed.execution.id,
        "approvalId": approval_id,
        "status": completed.execution.status,
        "execution": completed.execution,
        "preapproved": true,
        "message": "A pre-approval covered this request: the remediation action ran without waiting for approval.",
    }))
}

async fn tell(
    state: &AppState,
    ctx: &IamContext,
    approval_id: &Id,
    completed: &OperationExecution,
) -> Result<()> {
    let repository = state.alerting.remediation.repository();

    let Some(run) = repository
        .find_run_by_approval(&ctx.org_id, approval_id)
        .await?
    else {
        return Ok(());
    };

    let approval = state
        .agent
        .repository
        .get_approval(&ctx.org_id, approval_id)
        .await?;

    let approvers = reviewers(&approval.reviews);

    activity_audit::record(
        state,
        ctx,
        "remediation.preapproved_run",
        "incident",
        &run.incident_id.0,
        json!({
            "action": run.action_name,
            "approvalId": approval_id,
            "runId": run.id,
            "approvers": approvers,
            "status": completed.execution.status,
        }),
    )
    .await;

    let channels = repository
        .get_action(&ctx.org_id, &run.action_id)
        .await?
        .and_then(|action| action.preapproval)
        .map(|preapproval| preapproval.channels)
        .unwrap_or_default();

    if channels.is_empty() {
        return Ok(());
    }

    let incident = state
        .alerting
        .service
        .get_incident(&run.incident_id)
        .await?;

    let language = state
        .iam
        .workspace_preference_defaults
        .get(&ctx.org_id)
        .await
        .map(|defaults| defaults.language)
        .unwrap_or_else(|_| FALLBACK_LANGUAGE_CODE.to_string());

    let pack = state
        .platform
        .i18n_language
        .load_pack(LANGUAGE_KEY_PREFIX, &language)
        .await?;

    let mut approver_names = Vec::new();

    for approver in &approvers {
        approver_names.push(name(state, approver).await);
    }

    let base = state.platform.external_url.trim().trim_end_matches('/');
    let requester = name(state, &run.requested_by).await;
    let approver_names = approver_names.join(", ");

    let facts = RunFacts {
        action: &run.action_name,
        incident: &incident.summary,
        requester: &requester,
        approvers: &approver_names,
        succeeded: completed.execution.status == ExecutionStatus::Succeeded,
        result: completed
            .execution
            .output_summary
            .as_deref()
            .or(completed.execution.error.as_deref())
            .unwrap_or_default(),
        url: (!base.is_empty()).then(|| format!("{base}/alerts/incidents/{}", incident.id)),
    };

    let message = notice_message(&pack, &facts)?;

    for channel in channels {
        let idempotency_key = format!(
            "remediation_preapproval:{}:{}:{}:{}",
            run.id.0,
            channel.connector_id.0,
            channel.target_type.as_str(),
            blake3::hash(channel.target.as_bytes()).to_hex()
        );

        let send = DirectSend {
            connector_id: channel.connector_id.clone(),
            target: NotifyTarget {
                target_type: channel.target_type,
                value: channel.target.clone(),
                metadata: Default::default(),
            },
            message: message.clone(),
            event_id: format!("remediation.preapproved_run:{}", run.id.0),
            idempotency_key,
        };

        let outcome = state.alerting.notify.send_direct(&ctx.org_id, send).await;

        if !outcome.delivered {
            tracing::warn!(
                run_id = %run.id,
                target = %outcome.target_masked,
                error = outcome.error.as_deref().unwrap_or_default(),
                "a pre-approved run could not be told on a channel"
            );
        }
    }

    Ok(())
}

/// The people who approved, as the reviews name them.
fn reviewers(reviews: &Value) -> Vec<Id> {
    reviews
        .as_array()
        .into_iter()
        .flatten()
        .filter(|review| review["decision"] == "approved")
        .filter_map(|review| review["reviewerId"].as_str())
        .map(Id::from_string)
        .collect()
}

/// A person's name, else their email, else their ID.
async fn name(state: &AppState, user_id: &Id) -> String {
    match state.iam.service.users.get(user_id).await {
        Ok(user) if !user.display_name.trim().is_empty() => user.display_name,
        Ok(user) => user.email,
        Err(_) => user_id.to_string(),
    }
}

/// What the notification says of one run.
struct RunFacts<'a> {
    action: &'a str,
    incident: &'a str,
    requester: &'a str,
    approvers: &'a str,
    succeeded: bool,
    result: &'a str,
    url: Option<String>,
}

fn notice_message(pack: &LanguagePack, facts: &RunFacts<'_>) -> Result<NotifyMessage> {
    let fills = [
        ("action", facts.action),
        ("incident", facts.incident),
        ("requester", facts.requester),
        ("approvers", facts.approvers),
        ("result", facts.result),
    ];

    let title_key = if facts.succeeded {
        "remediation.preapproval.executed.succeeded_title"
    } else {
        "remediation.preapproval.executed.failed_title"
    };

    let title = pack.render(title_key, &fills)?;
    let mut text = pack.render("remediation.preapproval.executed.text", &fills)?;

    if let Some(url) = &facts.url {
        text.push('\n');
        text.push_str(&pack.render(
            "remediation.preapproval.executed.open",
            &[("url", url.as_str())],
        )?);
    }

    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert("source".to_string(), "remediation_preapproval".to_string());
    metadata.insert(
        "result".to_string(),
        if facts.succeeded {
            "succeeded"
        } else {
            "failed"
        }
        .to_string(),
    );

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n_language_seed::seeded_pack;

    fn facts(succeeded: bool) -> RunFacts<'static> {
        RunFacts {
            action: "Restart checkout",
            incident: "Checkout errors",
            requester: "Mole Agent run by Kenji",
            approvers: "Alice, Bob",
            succeeded,
            result: "HTTP 201: queued",
            url: Some("https://ms.example.com/alerts/incidents/inc-1".into()),
        }
    }

    #[test]
    fn the_notice_says_whether_it_worked_in_both_languages() {
        for language in ["en-us", "zh-cn"] {
            let pack = seeded_pack(LANGUAGE_KEY_PREFIX, language);

            let ran = notice_message(&pack, &facts(true)).unwrap();
            let failed = notice_message(&pack, &facts(false)).unwrap();

            assert_ne!(ran.title, failed.title, "{language}");
            assert!(ran.title.contains("Restart checkout"), "{language}");
            assert!(ran.text.contains("Alice, Bob"), "{language}");
            assert!(ran.text.contains("HTTP 201: queued"), "{language}");
            assert!(
                ran.text
                    .contains("https://ms.example.com/alerts/incidents/inc-1"),
                "{language}"
            );
            assert_eq!(failed.metadata["result"], "failed");
        }
    }

    #[test]
    fn only_a_pre_approved_approval_runs_at_once() {
        let approval = |status: &str, reviews: Value| {
            ToolResult::json(json!({
                "approval": {"id": "appr-1", "status": status, "reviews": reviews},
            }))
        };

        let given = json!([{"reviewerId": "u-1", "decision": "approved", "via": "preapproval"}]);

        assert_eq!(
            preapproved_in(&approval("approved", given.clone())),
            Some(Id::from_string("appr-1"))
        );
        assert_eq!(preapproved_in(&approval("pending", json!([]))), None);
        assert_eq!(
            preapproved_in(&approval(
                "approved",
                json!([{"reviewerId": "u-1", "decision": "approved"}])
            )),
            None
        );
        assert_eq!(reviewers(&given), vec![Id::from_string("u-1")]);
    }
}
