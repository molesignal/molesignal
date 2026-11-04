// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::json;
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};
use url::Url;

use super::{
    ToolRuntime,
    common::{bounded, json_result, parse_args},
};
use crate::{
    app::iam::IamContext,
    domain::status_page::{
        AutomationCandidateState, StatusPageEventView, StatusPageIncidentKind, StatusPageLifecycle,
        StatusPageSubscriberChannel,
    },
    shared::{Error, Result, ids::Id},
};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusPageArgs {
    page_id: Option<String>,
    event_id: Option<String>,
    lifecycle: Option<StatusPageLifecycle>,
    kind: Option<StatusPageIncidentKind>,
    view: Option<StatusPageEventView>,
    state: Option<AutomationCandidateState>,
    page: Option<u32>,
    limit: Option<usize>,
}

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: serde_json::Value,
) -> Result<ToolResult> {
    let args: StatusPageArgs = parse_args(arguments)?;

    match kind {
        BuiltinToolKind::ListStatusPages => {
            let pages = runtime
                .status_pages
                .list_pages(&auth.org_id, args.lifecycle)
                .await?;

            json_result(&bounded(pages, args.limit, 500))
        }
        BuiltinToolKind::GetStatusPage => {
            let page_id = required_id(args.page_id, "pageId")?;

            json_result(
                &runtime
                    .status_pages
                    .get_snapshot(&auth.org_id, &page_id)
                    .await?,
            )
        }
        BuiltinToolKind::ListStatusPageIncidents => {
            let page_id = required_id(args.page_id, "pageId")?;

            json_result(
                &runtime
                    .status_pages
                    .list_events(
                        &auth.org_id,
                        &page_id,
                        args.kind.unwrap_or(StatusPageIncidentKind::Incident),
                        args.view.unwrap_or(StatusPageEventView::Current),
                    )
                    .await?,
            )
        }
        BuiltinToolKind::GetStatusPageIncident => {
            let page_id = required_id(args.page_id, "pageId")?;
            let event_id = required_id(args.event_id, "eventId")?;

            json_result(
                &runtime
                    .status_pages
                    .get_event(&auth.org_id, &page_id, &event_id)
                    .await?,
            )
        }
        BuiltinToolKind::ListStatusPageSubscribers => {
            let page_id = required_id(args.page_id, "pageId")?;

            let page = runtime
                .status_pages
                .list_subscribers(&auth.org_id, &page_id, args.page.unwrap_or(1))
                .await?;

            let items = page
                .items
                .into_iter()
                .map(|subscriber| {
                    json!({
                        "id": subscriber.id,
                        "channel": subscriber.channel,
                        "maskedTarget": mask_target(subscriber.channel, &subscriber.target),
                        "targetLabel": subscriber.target_label,
                        "status": subscriber.status,
                        "componentScope": subscriber.components.scope,
                        "componentIds": subscriber.components.component_ids,
                        "origin": subscriber.origin,
                        "confirmationSentAt": subscriber.confirmation_sent_at,
                        "confirmedAt": subscriber.confirmed_at,
                        "unsubscribedAt": subscriber.unsubscribed_at,
                        "createdAt": subscriber.created_at,
                        "updatedAt": subscriber.updated_at,
                    })
                })
                .collect::<Vec<_>>();

            Ok(ToolResult::json(json!({
                "items": items,
                "page": page.page,
                "perPage": page.per_page,
                "total": page.total,
            })))
        }
        BuiltinToolKind::ListStatusPageDeliveries => {
            let page_id = required_id(args.page_id, "pageId")?;

            let page = runtime
                .status_pages
                .list_notification_deliveries(&auth.org_id, &page_id, args.page.unwrap_or(1))
                .await?;

            let items = page
                .items
                .into_iter()
                .map(|delivery| {
                    json!({
                        "id": delivery.id,
                        "subscriberId": delivery.subscriber_id,
                        "channel": delivery.channel,
                        "maskedTarget": mask_target(delivery.channel, &delivery.target),
                        "eventKey": delivery.event_key,
                        "status": delivery.status.as_str(),
                        "attempts": delivery.attempts,
                        "nextAttemptAt": delivery.next_attempt_at,
                        "lastError": delivery.last_error,
                        "deliveredAt": delivery.delivered_at,
                        "createdAt": delivery.created_at,
                        "updatedAt": delivery.updated_at,
                    })
                })
                .collect::<Vec<_>>();

            Ok(ToolResult::json(json!({
                "items": items,
                "page": page.page,
                "perPage": page.per_page,
                "total": page.total,
            })))
        }
        BuiltinToolKind::ListStatusPageAutomationRules => {
            let page_id = required_id(args.page_id, "pageId")?;

            json_result(
                &runtime
                    .status_pages
                    .list_automation_rules(&auth.org_id, &page_id)
                    .await?,
            )
        }
        BuiltinToolKind::ListStatusPageAutomationCandidates => {
            let page_id = required_id(args.page_id, "pageId")?;
            let limit = args.limit.unwrap_or(50).clamp(1, 100);

            json_result(
                &runtime
                    .status_pages
                    .list_automation_candidates(
                        &auth.org_id,
                        &page_id,
                        args.state,
                        u32::try_from(limit).unwrap_or(100),
                    )
                    .await?,
            )
        }
        BuiltinToolKind::GetStatusPageAutomationSettings => {
            let page_id = required_id(args.page_id, "pageId")?;

            Ok(ToolResult::json(serde_json::json!({
                "settings": runtime
                    .status_pages
                    .get_automation_settings(&auth.org_id, &page_id)
                    .await?,
            })))
        }
        _ => unreachable!("status-page executor received unrelated tool"),
    }
}

fn required_id(value: Option<String>, field: &str) -> Result<Id> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(Id)
        .ok_or_else(|| Error::invalid(format!("{field} is required")))
}

fn mask_target(channel: StatusPageSubscriberChannel, target: &str) -> String {
    match channel {
        StatusPageSubscriberChannel::Email => {
            let Some((local, domain)) = target.rsplit_once('@') else {
                return "***".into();
            };

            format!("{}***@{domain}", local.chars().next().unwrap_or('*'))
        }
        StatusPageSubscriberChannel::Webhook | StatusPageSubscriberChannel::Slack => {
            Url::parse(target)
                .ok()
                .and_then(|url| {
                    let host = url.host_str()?;

                    let port = url
                        .port()
                        .map(|port| format!(":{port}"))
                        .unwrap_or_default();

                    Some(format!("{}://{host}{port}/***", url.scheme()))
                })
                .unwrap_or_else(|| "***".into())
        }
    }
}
