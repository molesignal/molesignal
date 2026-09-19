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
    domain::alerting::incident_group::IncidentGroupState,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListAlertRules => {
            let args: ListEnabledArgs = parse_args(arguments)?;
            let mut rules = runtime.alerting.service.list_rules(&auth.org_id).await?;
            if args.enabled_only {
                rules.retain(|rule| rule.enabled);
            }
            json_result(&bounded(rules, args.limit, 500))
        }
        BuiltinToolKind::GetAlertRule => {
            let args: IdArg = parse_args(arguments)?;
            let rule = runtime
                .alerting
                .service
                .get_rule(&Id(args.rule_id()?))
                .await?;
            ensure_org(&rule.org_id, auth)?;
            json_result(&rule)
        }
        BuiltinToolKind::TestAlertRule => {
            let args: IdArg = parse_args(arguments)?;
            let rule = runtime
                .alerting
                .service
                .get_rule(&Id(args.rule_id()?))
                .await?;
            ensure_org(&rule.org_id, auth)?;
            json_result(
                &runtime
                    .alerting
                    .evaluator
                    .test_rule(&rule, TimestampMicros::now())
                    .await?,
            )
        }
        BuiltinToolKind::ListIncidentGroups => {
            let args: IncidentGroupArgs = parse_args(arguments)?;
            let state = args.status.as_deref().map(parse_group_state).transpose()?;
            json_result(
                &runtime
                    .alerting
                    .incident_groups
                    .list(
                        &auth.org_id,
                        state,
                        args.limit.unwrap_or(100).clamp(1, 500) as i64,
                    )
                    .await?,
            )
        }
        BuiltinToolKind::GetIncidentGroup => {
            let args: IdArg = parse_args(arguments)?;
            json_result(
                &runtime
                    .alerting
                    .incident_groups
                    .get(&auth.org_id, &Id(args.group_id()?))
                    .await?,
            )
        }
        BuiltinToolKind::ListMuteRules => {
            let args: ListEnabledArgs = parse_args(arguments)?;
            let mut rules = runtime.alerting.mute_rules.list(&auth.org_id).await?;
            if args.enabled_only {
                rules.retain(|rule| rule.enabled);
            }
            json_result(&bounded(rules, args.limit, 500))
        }
        BuiltinToolKind::GetMuteRule => {
            let args: IdArg = parse_args(arguments)?;
            let rule = runtime
                .alerting
                .mute_rules
                .get(&Id(args.mute_id()?))
                .await?;
            ensure_org(&rule.org_id, auth)?;
            json_result(&rule)
        }
        BuiltinToolKind::ListEscalationPolicies => {
            let args: ListArgs = parse_args(arguments)?;
            let policies = runtime.alerting.service.list_policies(&auth.org_id).await?;
            json_result(&bounded(policies, args.limit, 500))
        }
        BuiltinToolKind::GetEscalationPolicy => {
            let args: IdArg = parse_args(arguments)?;
            let policy = runtime
                .alerting
                .service
                .get_policy(&Id(args.policy_id()?))
                .await?;
            ensure_org(&policy.org_id, auth)?;
            json_result(&policy)
        }
        _ => unreachable!("alerting handler received unrelated tool"),
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
struct ListEnabledArgs {
    #[serde(default)]
    enabled_only: bool,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct IncidentGroupArgs {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArg {
    rule_id: Option<String>,
    group_id: Option<String>,
    mute_id: Option<String>,
    policy_id: Option<String>,
}

impl IdArg {
    fn value(&self, value: &Option<String>, field: &str) -> Result<String> {
        value
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .ok_or_else(|| Error::invalid(format!("{field} is required")))
    }

    fn rule_id(&self) -> Result<String> {
        self.value(&self.rule_id, "rule_id")
    }
    fn group_id(&self) -> Result<String> {
        self.value(&self.group_id, "group_id")
    }
    fn mute_id(&self) -> Result<String> {
        self.value(&self.mute_id, "mute_id")
    }
    fn policy_id(&self) -> Result<String> {
        self.value(&self.policy_id, "policy_id")
    }
}

fn parse_group_state(value: &str) -> Result<IncidentGroupState> {
    match value {
        "open" => Ok(IncidentGroupState::Open),
        "acked" => Ok(IncidentGroupState::Acked),
        "resolved" => Ok(IncidentGroupState::Resolved),
        _ => Err(Error::invalid("status must be open, acked, or resolved")),
    }
}

fn ensure_org(org_id: &Id, auth: &IamContext) -> Result<()> {
    if org_id == &auth.org_id {
        Ok(())
    } else {
        Err(Error::not_found("resource not found"))
    }
}
