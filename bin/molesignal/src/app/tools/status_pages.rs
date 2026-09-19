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
            let page_id = required_id(args.page_id, "page_id")?;
            json_result(
                &runtime
                    .status_pages
                    .get_snapshot(&auth.org_id, &page_id)
                    .await?,
            )
        }
        BuiltinToolKind::ListStatusPageIncidents => {
            let page_id = required_id(args.page_id, "page_id")?;
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
            let page_id = required_id(args.page_id, "page_id")?;
            let event_id = required_id(args.event_id, "event_id")?;
            json_result(
                &runtime
                    .status_pages
                    .get_event(&auth.org_id, &page_id, &event_id)
                    .await?,
            )
        }
        BuiltinToolKind::ListStatusPageSubscribers => {
            let page_id = required_id(args.page_id, "page_id")?;
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
                        "masked_target": mask_target(subscriber.channel, &subscriber.target),
                        "status": subscriber.status,
                        "confirmation_sent_at": subscriber.confirmation_sent_at,
                        "confirmed_at": subscriber.confirmed_at,
                        "unsubscribed_at": subscriber.unsubscribed_at,
                        "created_at": subscriber.created_at,
                        "updated_at": subscriber.updated_at,
                    })
                })
                .collect::<Vec<_>>();
            Ok(ToolResult::json(json!({
                "items": items,
                "page": page.page,
                "per_page": page.per_page,
                "total": page.total,
            })))
        }
        BuiltinToolKind::ListStatusPageDeliveries => {
            let page_id = required_id(args.page_id, "page_id")?;
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
                        "subscriber_id": delivery.subscriber_id,
                        "channel": delivery.channel,
                        "masked_target": mask_target(delivery.channel, &delivery.target),
                        "event_key": delivery.event_key,
                        "status": delivery.status.as_str(),
                        "attempts": delivery.attempts,
                        "next_attempt_at": delivery.next_attempt_at,
                        "last_error": delivery.last_error,
                        "delivered_at": delivery.delivered_at,
                        "created_at": delivery.created_at,
                        "updated_at": delivery.updated_at,
                    })
                })
                .collect::<Vec<_>>();
            Ok(ToolResult::json(json!({
                "items": items,
                "page": page.page,
                "per_page": page.per_page,
                "total": page.total,
            })))
        }
        BuiltinToolKind::ListStatusPageAutomationRules => {
            let page_id = required_id(args.page_id, "page_id")?;
            json_result(
                &runtime
                    .status_pages
                    .list_automation_rules(&auth.org_id, &page_id)
                    .await?,
            )
        }
        BuiltinToolKind::ListStatusPageAutomationCandidates => {
            let page_id = required_id(args.page_id, "page_id")?;
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
            let page_id = required_id(args.page_id, "page_id")?;
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
        StatusPageSubscriberChannel::Webhook => Url::parse(target)
            .ok()
            .and_then(|url| {
                let host = url.host_str()?;
                let port = url
                    .port()
                    .map(|port| format!(":{port}"))
                    .unwrap_or_default();
                Some(format!("{}://{host}{port}/***", url.scheme()))
            })
            .unwrap_or_else(|| "***".into()),
    }
}
