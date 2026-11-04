// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 保存规则时的校验：名称、任务提示、触发、输出与时限。通知渠道还要核对连接器，那一步在
//! 路由里（要查这个组织的连接器）。

use super::schedule;
use crate::{
    agent::automations::{
        AutomationOutputs, AutomationTrigger, FindingFilter, IncidentFilter, MAX_CHANNELS,
        MAX_PROMPT_CHARS, MAX_TIMEOUT_SECS, MIN_TIMEOUT_SECS,
    },
    domain::{alerting::discovery::FindingKind, notify::connector::NotifyTargetType},
    shared::{Error, Result},
};

pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_DESCRIPTION_CHARS: usize = 1_000;
/// 事件筛选每一项最多这么多个值。
const MAX_FILTER_VALUES: usize = 50;
const MAX_FILTER_VALUE_CHARS: usize = 200;
const MAX_TARGET_CHARS: usize = 500;
const SEVERITIES: [&str; 4] = ["info", "warning", "error", "critical"];

fn invalid(message: impl Into<String>) -> Error {
    Error::invalid(message.into())
}

/// 去掉首尾空白后的名称；不能为空、不能太长、不能带控制字符。
pub fn name(raw: &str) -> Result<String> {
    let name = raw.trim();

    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(invalid(format!(
            "an automation's name must have 1 to {MAX_NAME_CHARS} characters"
        )));
    }

    if name.chars().any(char::is_control) {
        return Err(invalid(
            "an automation's name cannot contain control characters",
        ));
    }

    Ok(name.to_string())
}

pub fn description(raw: &str) -> Result<String> {
    let description = raw.trim();

    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(invalid(format!(
            "a description has at most {MAX_DESCRIPTION_CHARS} characters"
        )));
    }

    Ok(description.to_string())
}

pub fn prompt(raw: &str) -> Result<String> {
    let prompt = raw.trim();

    if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(invalid(format!(
            "the task must have 1 to {MAX_PROMPT_CHARS} characters"
        )));
    }

    Ok(prompt.to_string())
}

pub fn timeout(secs: Option<u32>) -> Result<u32> {
    let secs = secs.unwrap_or(crate::agent::automations::DEFAULT_TIMEOUT_SECS);

    if !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&secs) {
        return Err(invalid(format!(
            "timeout_secs must be between {MIN_TIMEOUT_SECS} and {MAX_TIMEOUT_SECS}"
        )));
    }

    Ok(secs)
}

fn values(field: &str, values: &[String]) -> Result<Vec<String>> {
    if values.len() > MAX_FILTER_VALUES {
        return Err(invalid(format!(
            "{field} holds at most {MAX_FILTER_VALUES} values"
        )));
    }

    let mut kept = Vec::new();

    for value in values.iter().map(|value| value.trim()) {
        if value.is_empty() || value.chars().count() > MAX_FILTER_VALUE_CHARS {
            return Err(invalid(format!(
                "every value of {field} has 1 to {MAX_FILTER_VALUE_CHARS} characters"
            )));
        }

        if !kept.iter().any(|kept: &String| kept == value) {
            kept.push(value.to_string());
        }
    }

    Ok(kept)
}

fn incident_filter(filter: &IncidentFilter) -> Result<IncidentFilter> {
    let lowered = filter
        .severities
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let severities = values("severities", &lowered)?;

    if let Some(unknown) = severities
        .iter()
        .find(|value| !SEVERITIES.contains(&value.as_str()))
    {
        return Err(invalid(format!(
            "`{unknown}` is not a severity; use info, warning, error or critical"
        )));
    }

    if filter.labels.len() > MAX_FILTER_VALUES {
        return Err(invalid(format!(
            "labels holds at most {MAX_FILTER_VALUES} pairs"
        )));
    }

    let mut labels = std::collections::BTreeMap::new();

    for (key, value) in &filter.labels {
        let (key, value) = (key.trim(), value.trim());

        if key.is_empty()
            || key.chars().count() > MAX_FILTER_VALUE_CHARS
            || value.chars().count() > MAX_FILTER_VALUE_CHARS
        {
            return Err(invalid(format!(
                "a label's name has 1 to {MAX_FILTER_VALUE_CHARS} characters, its value at most as many"
            )));
        }

        labels.insert(key.to_string(), value.to_string());
    }

    Ok(IncidentFilter {
        severities,
        rule_ids: values("rule_ids", &filter.rule_ids)?,
        services: values("services", &filter.services)?,
        labels,
    })
}

/// 发现筛选说得通：种类认得出，严重度是四档之一。
fn finding_filter(filter: &FindingFilter) -> Result<FindingFilter> {
    let kinds = values("kinds", &filter.kinds)?;

    if let Some(unknown) = kinds.iter().find(|kind| FindingKind::parse(kind).is_none()) {
        return Err(invalid(format!(
            "`{unknown}` is not a kind of discovery finding"
        )));
    }

    let min_severity = filter
        .min_severity
        .as_deref()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());

    if let Some(severity) = &min_severity
        && !SEVERITIES.contains(&severity.as_str())
    {
        return Err(invalid(format!(
            "`{severity}` is not a severity; use info, warning, error or critical"
        )));
    }

    Ok(FindingFilter {
        kinds,
        min_severity,
        subjects: values("subjects", &filter.subjects)?,
    })
}

/// 触发说得通；返回整理过的触发（去掉首尾空白与重复值）。
pub fn trigger(trigger: &AutomationTrigger) -> Result<AutomationTrigger> {
    Ok(match trigger {
        AutomationTrigger::Schedule { cron, timezone } => {
            let (cron, timezone) = (cron.trim(), timezone.trim());
            schedule::validate(cron, timezone)?;

            AutomationTrigger::Schedule {
                cron: cron.to_string(),
                timezone: timezone.to_string(),
            }
        }
        AutomationTrigger::Incident(filter) => {
            AutomationTrigger::Incident(incident_filter(filter)?)
        }
        AutomationTrigger::Finding(filter) => AutomationTrigger::Finding(finding_filter(filter)?),
        AutomationTrigger::Api => AutomationTrigger::Api,
        AutomationTrigger::Manual => AutomationTrigger::Manual,
    })
}

/// 输出说得通：渠道不太多、是固定地址或固定群组；只有事件触发的规则能写 RCA。
pub fn outputs(
    outputs: &AutomationOutputs,
    trigger: &AutomationTrigger,
) -> Result<AutomationOutputs> {
    if outputs.channels.len() > MAX_CHANNELS {
        return Err(invalid(format!(
            "an automation sends to at most {MAX_CHANNELS} channels"
        )));
    }

    let mut channels = Vec::new();

    for channel in &outputs.channels {
        if !matches!(
            channel.target_type,
            NotifyTargetType::FixedAddress | NotifyTargetType::FixedGroup
        ) {
            return Err(invalid(
                "a channel is a fixed address or a fixed group on a notify connector",
            ));
        }

        let target = channel.target.trim();

        if target.is_empty() || target.chars().count() > MAX_TARGET_CHARS {
            return Err(invalid(format!(
                "a channel's address has 1 to {MAX_TARGET_CHARS} characters"
            )));
        }

        let mut channel = channel.clone();
        channel.target = target.to_string();

        if !channels.contains(&channel) {
            channels.push(channel);
        }
    }

    if outputs.write_rca && !matches!(trigger, AutomationTrigger::Incident(_)) {
        return Err(invalid(
            "only an automation started by incidents can write an incident's RCA",
        ));
    }

    Ok(AutomationOutputs {
        channels,
        write_rca: outputs.write_rca,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{agent::automations::AutomationChannel, shared::ids::Id};

    #[test]
    fn names_prompts_and_timeouts_have_bounds() {
        assert_eq!(name("  Daily health check ").unwrap(), "Daily health check");
        assert!(name("   ").is_err());
        assert!(name(&"x".repeat(MAX_NAME_CHARS + 1)).is_err());
        assert!(name("a\u{7}b").is_err());
        assert!(prompt("").is_err());
        assert_eq!(timeout(None).unwrap(), 600);
        assert!(timeout(Some(30)).is_err());
        assert!(timeout(Some(3_601)).is_err());
    }

    #[test]
    fn incident_filters_are_tidied_and_severities_checked() {
        let tidy = trigger(&AutomationTrigger::Incident(IncidentFilter {
            severities: vec!["Critical".into(), "critical".into()],
            rule_ids: Vec::new(),
            services: vec![" checkout ".into()],
            labels: BTreeMap::from([(" env ".into(), " prod ".into())]),
        }))
        .unwrap();

        let AutomationTrigger::Incident(filter) = tidy else {
            panic!("an incident trigger");
        };
        assert_eq!(filter.severities, vec!["critical".to_string()]);
        assert_eq!(filter.services, vec!["checkout".to_string()]);
        assert_eq!(filter.labels.get("env").map(String::as_str), Some("prod"));

        assert!(
            trigger(&AutomationTrigger::Incident(IncidentFilter {
                severities: vec!["urgent".into()],
                ..IncidentFilter::default()
            }))
            .is_err()
        );
    }

    #[test]
    fn only_incident_automations_write_an_rca_and_channels_are_fixed_targets() {
        let channel = AutomationChannel {
            connector_id: Id::from_string("conn-1"),
            target_type: NotifyTargetType::FixedGroup,
            target: " oc_ops ".into(),
        };
        let wanted = AutomationOutputs {
            channels: vec![channel.clone(), channel.clone()],
            write_rca: true,
        };

        let incident = AutomationTrigger::Incident(IncidentFilter::default());
        let kept = outputs(&wanted, &incident).unwrap();
        assert_eq!(kept.channels.len(), 1);
        assert_eq!(kept.channels[0].target, "oc_ops");

        assert!(outputs(&wanted, &AutomationTrigger::Manual).is_err());

        let personal = AutomationOutputs {
            channels: vec![AutomationChannel {
                target_type: NotifyTargetType::DirectUser,
                ..channel
            }],
            write_rca: false,
        };
        assert!(outputs(&personal, &AutomationTrigger::Manual).is_err());
    }
}
