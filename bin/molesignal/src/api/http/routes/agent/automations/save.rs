// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 新建与修改规则：校验提交的东西，核对通知渠道，算出下一次运行，记下执行身份与语言。

use serde::Deserialize;

use super::access::run_as;
use crate::{
    agent::{
        automations::{Automation, AutomationOutputs, AutomationTrigger, MAX_AUTOMATIONS_PER_ORG},
        scope::Scope,
    },
    api::{AppState, http::interface_language::interface_language},
    app::{
        automations::{schedule, templates, validate},
        iam::IamContext,
    },
    domain::notify::connector::NotifyTarget,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 保存规则时提交的东西。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutomationInput {
    pub(super) name: String,
    #[serde(default)]
    pub(super) description: String,
    pub(super) prompt: String,
    #[serde(default = "enabled")]
    pub(super) enabled: bool,
    pub(super) trigger: AutomationTrigger,
    #[serde(default)]
    pub(super) outputs: AutomationOutputs,
    #[serde(default)]
    pub(super) timeout_secs: Option<u32>,
    /// 新建时：`org`、`team`（带 `team_id`）或 `personal`。修改时不能换作用域。
    #[serde(default)]
    pub(super) scope: Option<String>,
    #[serde(default)]
    pub(super) team_id: Option<String>,
    /// 新建时：从哪个内置模板建的。
    #[serde(default)]
    pub(super) template: Option<String>,
}

fn enabled() -> bool {
    true
}

/// 新建时提交的作用域。
pub(super) fn scope_of(input: &AutomationInput, ctx: &IamContext) -> Result<Scope> {
    match input.scope.as_deref().unwrap_or("personal") {
        "org" => Ok(Scope::Org),
        "team" => input
            .team_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| Scope::Team {
                team_id: Id::from_string(id),
            })
            .ok_or_else(|| Error::invalid("a team automation needs team_id")),
        "personal" => Ok(Scope::Personal {
            owner_id: ctx.user_id.clone(),
        }),
        other => Err(Error::invalid(format!("unknown scope `{other}`"))),
    }
}

/// 下一次定时运行：只有启用中的定时规则有。
pub(super) fn next_run(
    trigger: &AutomationTrigger,
    enabled: bool,
    now: TimestampMicros,
) -> Result<Option<TimestampMicros>> {
    match trigger {
        AutomationTrigger::Schedule { cron, timezone } if enabled => {
            schedule::next_after(cron, timezone, now)
        }
        _ => Ok(None),
    }
}

/// 校验过、核对过渠道的字段。
struct Checked {
    name: String,
    description: String,
    prompt: String,
    trigger: AutomationTrigger,
    outputs: AutomationOutputs,
    timeout_secs: u32,
}

async fn check(state: &AppState, ctx: &IamContext, input: &AutomationInput) -> Result<Checked> {
    let trigger = validate::trigger(&input.trigger)?;
    let outputs = validate::outputs(&input.outputs, &trigger)?;

    for channel in &outputs.channels {
        let target = NotifyTarget {
            target_type: channel.target_type,
            value: channel.target.clone(),
            metadata: Default::default(),
        };

        match state
            .alerting
            .notify
            .validate_direct_target(&ctx.org_id, &channel.connector_id, &target)
            .await
        {
            Err(Error::NotFound(_)) => {
                return Err(Error::invalid(format!(
                    "notify connector `{}` does not exist in this organization",
                    channel.connector_id.0
                )));
            }
            checked => {
                checked?;
            }
        }
    }

    Ok(Checked {
        name: validate::name(&input.name)?,
        description: validate::description(&input.description)?,
        prompt: validate::prompt(&input.prompt)?,
        trigger,
        outputs,
        timeout_secs: validate::timeout(input.timeout_secs)?,
    })
}

/// 新建：作用域已经核对过能不能建。
pub(super) async fn create(
    state: &AppState,
    ctx: &IamContext,
    scope: Scope,
    input: AutomationInput,
) -> Result<Automation> {
    let run_as = run_as(ctx)?;

    if state.agent.automations.count(&ctx.org_id).await? >= MAX_AUTOMATIONS_PER_ORG {
        return Err(Error::invalid(format!(
            "an organization has at most {MAX_AUTOMATIONS_PER_ORG} automations"
        )));
    }

    if let Some(template) = input.template.as_deref()
        && !templates::is_known(template)
    {
        return Err(Error::invalid(format!("unknown template `{template}`")));
    }

    let checked = check(state, ctx, &input).await?;
    let now = TimestampMicros::now();

    let automation = Automation {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        scope,
        name: checked.name,
        description: checked.description,
        prompt: checked.prompt,
        enabled: input.enabled,
        next_run_at: next_run(&checked.trigger, input.enabled, now)?,
        // 事件、发现触发只看保存之后的新事件、新发现。
        incident_cursor: Some(now),
        trigger: checked.trigger,
        outputs: checked.outputs,
        run_as_user_id: run_as,
        language: interface_language(state, ctx).await?,
        timeout_secs: checked.timeout_secs,
        template: input.template,
        api_token_prefix: None,
        last_run: None,
        created_by: ctx.user_id.clone(),
        updated_by: ctx.user_id.clone(),
        created_at: now,
        updated_at: now,
    };

    state.agent.automations.create(automation).await
}

/// 修改：作用域不变，保存的人成为新的执行身份。
pub(super) async fn update(
    state: &AppState,
    ctx: &IamContext,
    existing: Automation,
    input: AutomationInput,
) -> Result<Automation> {
    if input.scope.is_some() || input.team_id.is_some() {
        return Err(Error::invalid(
            "an automation keeps its scope; clone it to use another scope",
        ));
    }

    let run_as = run_as(ctx)?;
    let checked = check(state, ctx, &input).await?;
    let now = TimestampMicros::now();

    // 换了触发方式、或重新启用时，从现在起的新事件、新发现才算。
    let fresh_incidents =
        existing.trigger.kind() != input.trigger.kind() || (input.enabled && !existing.enabled);

    let automation = Automation {
        name: checked.name,
        description: checked.description,
        prompt: checked.prompt,
        enabled: input.enabled,
        next_run_at: next_run(&checked.trigger, input.enabled, now)?,
        incident_cursor: if fresh_incidents {
            Some(now)
        } else {
            existing.incident_cursor
        },
        trigger: checked.trigger,
        outputs: checked.outputs,
        run_as_user_id: run_as,
        language: interface_language(state, ctx).await?,
        timeout_secs: checked.timeout_secs,
        updated_by: ctx.user_id.clone(),
        updated_at: now,
        ..existing
    };

    state.agent.automations.update(automation).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_enabled_schedule_has_a_next_run() {
        let schedule = AutomationTrigger::Schedule {
            cron: "0 9 * * *".into(),
            timezone: "UTC".into(),
        };
        let now = TimestampMicros(1_760_000_000_000_000);

        assert!(next_run(&schedule, true, now).unwrap().unwrap().0 > now.0);
        assert_eq!(next_run(&schedule, false, now).unwrap(), None);
        assert_eq!(next_run(&AutomationTrigger::Api, true, now).unwrap(), None);
    }
}
