// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::Value;
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{
    ToolRuntime,
    common::{bounded, json_result, parse_args},
};
use crate::{
    app::iam::IamContext,
    domain::notify::delivery::{DeliveryFilter, DeliveryStatus},
    shared::{Error, Result, ids::Id},
};

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListNotificationConnectors => {
            let args: ListArgs = parse_args(arguments)?;
            let connectors = runtime
                .alerting
                .notify
                .list_connectors(&auth.org_id)
                .await?;
            json_result(&bounded(connectors, args.limit, 500))
        }
        BuiltinToolKind::GetNotificationConnector => {
            let args: IdArg = parse_args(arguments)?;
            json_result(
                &runtime
                    .alerting
                    .notify
                    .get_connector(&auth.org_id, &Id(args.required("connector_id")?))
                    .await?,
            )
        }
        BuiltinToolKind::ListNotificationPolicies => {
            let args: ListArgs = parse_args(arguments)?;
            let policies = runtime
                .alerting
                .notify_engine
                .list_policies(&auth.org_id)
                .await?;
            json_result(&bounded(policies, args.limit, 500))
        }
        BuiltinToolKind::GetNotificationPolicy => {
            let args: IdArg = parse_args(arguments)?;
            json_result(
                &runtime
                    .alerting
                    .notify_engine
                    .get_policy(&auth.org_id, &Id(args.required("policy_id")?))
                    .await?,
            )
        }
        BuiltinToolKind::ListNotificationTemplates => {
            let args: ListArgs = parse_args(arguments)?;
            let templates = runtime.alerting.notify_templates.list(&auth.org_id).await?;
            json_result(&bounded(templates, args.limit, 500))
        }
        BuiltinToolKind::GetNotificationTemplate => {
            let args: IdArg = parse_args(arguments)?;
            json_result(
                &runtime
                    .alerting
                    .notify_templates
                    .get(&auth.org_id, &Id(args.required("template_id")?))
                    .await?,
            )
        }
        BuiltinToolKind::ListNotificationDeliveries => {
            let args: DeliveryArgs = parse_args(arguments)?;
            let status = args
                .status
                .as_deref()
                .map(parse_delivery_status)
                .transpose()?;
            json_result(
                &runtime
                    .alerting
                    .notify
                    .list_deliveries(
                        &auth.org_id,
                        &DeliveryFilter {
                            event_id: args.event_id,
                            status,
                            limit: args.limit.unwrap_or(50).clamp(1, 200) as u32,
                            ..DeliveryFilter::default()
                        },
                    )
                    .await?,
            )
        }
        BuiltinToolKind::GetNotificationDelivery => {
            let args: IdArg = parse_args(arguments)?;
            json_result(
                &runtime
                    .alerting
                    .notify
                    .get_delivery(&auth.org_id, &Id(args.required("delivery_id")?))
                    .await?,
            )
        }
        _ => unreachable!("notification handler received unrelated tool"),
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryArgs {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArg {
    connector_id: Option<String>,
    policy_id: Option<String>,
    template_id: Option<String>,
    delivery_id: Option<String>,
}

impl IdArg {
    fn required(&self, field: &str) -> Result<String> {
        let value = match field {
            "connector_id" => &self.connector_id,
            "policy_id" => &self.policy_id,
            "template_id" => &self.template_id,
            "delivery_id" => &self.delivery_id,
            _ => unreachable!(),
        };
        value
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .ok_or_else(|| Error::invalid(format!("{field} is required")))
    }
}

fn parse_delivery_status(value: &str) -> Result<DeliveryStatus> {
    match value {
        "pending" => Ok(DeliveryStatus::Pending),
        "sending" => Ok(DeliveryStatus::Sending),
        "success" => Ok(DeliveryStatus::Success),
        "failed" => Ok(DeliveryStatus::Failed),
        "skipped" => Ok(DeliveryStatus::Skipped),
        "acknowledged" => Ok(DeliveryStatus::Acknowledged),
        _ => Err(Error::invalid("invalid notification delivery status")),
    }
}
