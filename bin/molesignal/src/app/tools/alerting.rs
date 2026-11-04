// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{
    ToolRuntime,
    common::{bounded, json_result, parse_args},
};
use crate::{
    app::iam::IamContext,
    domain::alerting::{
        discovery::{FindingFilter, FindingKind},
        incident_group::IncidentGroupState,
    },
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
        BuiltinToolKind::ListInhibitRules => {
            let args: ListEnabledArgs = parse_args(arguments)?;
            let inhibitions = &runtime.alerting.inhibitions;
            let mut rules = inhibitions.rules().list(&auth.org_id).await?;
            let counts = inhibitions.counts(&auth.org_id).await?;

            if args.enabled_only {
                rules.retain(|rule| rule.enabled);
            }

            let rows: Vec<Value> = bounded(rules, args.limit, 500)
                .iter()
                .map(|rule| {
                    json!({
                        "rule": rule,
                        "inhibiting": counts.get(&rule.id).copied().unwrap_or(0),
                    })
                })
                .collect();

            json_result(&rows)
        }
        BuiltinToolKind::GetInhibitRule => {
            let args: IdArg = parse_args(arguments)?;

            let rule = runtime
                .alerting
                .inhibitions
                .rules()
                .get(&Id(args.inhibit_rule_id()?))
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
        BuiltinToolKind::ListDiscoveryFindings => list_findings(runtime, auth, arguments).await,
        BuiltinToolKind::GetDiscoveryFinding => {
            let args: FindingIdArgs = parse_args(arguments)?;
            let finding = runtime
                .alerting
                .discovery
                .get(&auth.org_id, &Id(args.finding_id))
                .await?
                .ok_or_else(|| Error::not_found("discovery finding"))?;

            json_result(&finding)
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListEnabledArgs {
    #[serde(default)]
    enabled_only: bool,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FindingArgs {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

/// 自动发现找到的问题，按条件列，最近看到的在前。
async fn list_findings(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: FindingArgs = parse_args(arguments)?;

    let open = match args.status.as_deref().unwrap_or("open") {
        "open" => Some(true),
        "ended" => Some(false),
        "all" => None,
        _ => return Err(Error::invalid("status must be open, ended, or all")),
    };

    let kind = args
        .kind
        .as_deref()
        .map(|kind| FindingKind::parse(kind).ok_or_else(|| Error::invalid("unknown finding kind")))
        .transpose()?;

    let findings = runtime
        .alerting
        .discovery
        .list(
            &auth.org_id,
            &FindingFilter {
                open,
                kind,
                since: None,
                limit: args.limit.unwrap_or(50).clamp(1, 200),
            },
        )
        .await?;

    json_result(&findings)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FindingIdArgs {
    finding_id: String,
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdArg {
    rule_id: Option<String>,
    group_id: Option<String>,
    mute_id: Option<String>,
    #[serde(default)]
    inhibit_rule_id: Option<String>,
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
        self.value(&self.rule_id, "ruleId")
    }

    fn group_id(&self) -> Result<String> {
        self.value(&self.group_id, "groupId")
    }

    fn inhibit_rule_id(&self) -> Result<String> {
        self.value(&self.inhibit_rule_id, "inhibitRuleId")
    }

    fn mute_id(&self) -> Result<String> {
        self.value(&self.mute_id, "muteId")
    }

    fn policy_id(&self) -> Result<String> {
        self.value(&self.policy_id, "policyId")
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
