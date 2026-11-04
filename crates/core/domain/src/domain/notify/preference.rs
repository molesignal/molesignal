// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    domain::alerting::importance::StepGroup,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyCategory {
    Alert,
    Oncall,
    Escalation,
    Report,
    Security,
    System,
}

impl NotifyCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Alert => "alert",
            Self::Oncall => "oncall",
            Self::Escalation => "escalation",
            Self::Report => "report",
            Self::Security => "security",
            Self::System => "system",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "alert" => Ok(Self::Alert),
            "oncall" => Ok(Self::Oncall),
            "escalation" => Ok(Self::Escalation),
            "report" => Ok(Self::Report),
            "security" => Ok(Self::Security),
            "system" => Ok(Self::System),
            other => Err(Error::internal(format!(
                "unknown notify preference category: {other}"
            ))),
        }
    }
}

/// 后续步骤最多等多久：一天。
pub const MAX_STEP_DELAY_SECS: i32 = 86_400;

/// 个人接收顺序里的一步。`delay_secs` 是这一步的等待时间：从第一次通知这个人算起，过了这么久仍没人
/// 确认告警，就用这一步再发一次。等待时间与上一步相同的一步只在上一步发送失败时使用。
///
/// 告警和升级两类各有高、低紧急度两组步骤（`urgency`）；组织的每档紧急度用其中一组。低紧急度的一组
/// 没设时，用它的告警只用高紧急度那组的第一步。其他类别只有高紧急度一组。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserNotifyPreferenceStep {
    pub id: Id,
    pub preference_id: Id,
    pub endpoint_id: Id,
    pub step_order: i32,
    #[serde(default)]
    pub delay_secs: i32,
    #[serde(default)]
    pub urgency: StepGroup,
    pub created_at: TimestampMicros,
}

/// 只有告警和升级两类有低紧急度的步骤。
pub fn has_low_urgency_steps(category: NotifyCategory) -> bool {
    matches!(category, NotifyCategory::Alert | NotifyCategory::Escalation)
}

impl UserNotifyPreference {
    /// 用这组步骤的通知用哪几步，按顺序：低紧急度的一组没设时只用高紧急度那组的第一步。
    pub fn steps_for(&self, group: StepGroup) -> Vec<&UserNotifyPreferenceStep> {
        let mut steps: Vec<&UserNotifyPreferenceStep> = self
            .steps
            .iter()
            .filter(|step| step.urgency == group)
            .collect();

        if steps.is_empty() && group == StepGroup::Low {
            steps = self
                .steps
                .iter()
                .filter(|step| step.urgency == StepGroup::High)
                .collect();
            steps.sort_by_key(|step| step.step_order);
            steps.truncate(1);
        }

        steps.sort_by_key(|step| step.step_order);
        steps
    }
}

/// 按步骤顺序检查各步的等待时间：第一步立即发送，后面的不能比前一步早，最多等一天。只有告警事件等人
/// 确认，所以只有告警类别能设等待时间。
pub fn validate_step_delays(category: NotifyCategory, delays_secs: &[i32]) -> Result<()> {
    if delays_secs.iter().all(|delay| *delay == 0) {
        return Ok(());
    }

    if category != NotifyCategory::Alert {
        return Err(Error::invalid(
            "only alert notifications wait for an acknowledgement: step delays belong to the alert category",
        ));
    }

    if delays_secs.first().is_some_and(|delay| *delay != 0) {
        return Err(Error::invalid(
            "the first notify step is sent at once: its delay must be 0",
        ));
    }

    if delays_secs
        .iter()
        .any(|delay| !(0..=MAX_STEP_DELAY_SECS).contains(delay))
    {
        return Err(Error::invalid(
            "a notify step waits between 0 seconds and one day",
        ));
    }

    if delays_secs.windows(2).any(|pair| pair[1] < pair[0]) {
        return Err(Error::invalid(
            "a notify step cannot wait less than the step before it",
        ));
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserNotifyPreference {
    pub id: Id,
    pub organization_id: Id,
    pub user_id: Id,
    pub category: NotifyCategory,
    pub enabled: bool,
    pub quiet_hours: Option<Value>,
    /// 免打扰时段里仍接收高紧急度的告警；低紧急度的一律不打扰。
    pub allow_high_urgency_bypass: bool,
    pub steps: Vec<UserNotifyPreferenceStep>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_steps_wait_at_least_as_long_as_the_one_before() {
        assert!(validate_step_delays(NotifyCategory::Alert, &[0, 180, 300]).is_ok());
        assert!(validate_step_delays(NotifyCategory::Alert, &[0, 0, 300]).is_ok());
        assert!(validate_step_delays(NotifyCategory::Alert, &[60, 180]).is_err());
        assert!(validate_step_delays(NotifyCategory::Alert, &[0, 300, 180]).is_err());
        assert!(
            validate_step_delays(NotifyCategory::Alert, &[0, MAX_STEP_DELAY_SECS + 1]).is_err()
        );
    }

    #[test]
    fn a_low_urgency_alert_uses_its_own_steps_or_the_first_high_one() {
        let step = |order: i32, urgency: StepGroup| UserNotifyPreferenceStep {
            id: Id::from_string(format!("{urgency:?}-{order}")),
            preference_id: Id::from_string("p"),
            endpoint_id: Id::from_string(format!("e{order}")),
            step_order: order,
            delay_secs: 0,
            urgency,
            created_at: TimestampMicros(0),
        };
        let mut preference = UserNotifyPreference {
            id: Id::from_string("p"),
            organization_id: Id::from_string("o"),
            user_id: Id::from_string("u"),
            category: NotifyCategory::Alert,
            enabled: true,
            quiet_hours: None,
            allow_high_urgency_bypass: true,
            steps: vec![step(2, StepGroup::High), step(1, StepGroup::High)],
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };

        let orders = |steps: Vec<&UserNotifyPreferenceStep>| -> Vec<(i32, StepGroup)> {
            steps
                .iter()
                .map(|step| (step.step_order, step.urgency))
                .collect()
        };

        assert_eq!(
            orders(preference.steps_for(StepGroup::High)),
            vec![(1, StepGroup::High), (2, StepGroup::High)]
        );
        assert_eq!(
            orders(preference.steps_for(StepGroup::Low)),
            vec![(1, StepGroup::High)]
        );

        preference.steps.push(step(1, StepGroup::Low));
        assert_eq!(
            orders(preference.steps_for(StepGroup::Low)),
            vec![(1, StepGroup::Low)]
        );
        assert!(has_low_urgency_steps(NotifyCategory::Escalation));
        assert!(!has_low_urgency_steps(NotifyCategory::Oncall));
    }

    #[test]
    fn only_alerts_wait_for_an_acknowledgement() {
        assert!(validate_step_delays(NotifyCategory::Oncall, &[0, 0]).is_ok());
        assert!(validate_step_delays(NotifyCategory::Oncall, &[0, 60]).is_err());
        assert!(validate_step_delays(NotifyCategory::Report, &[]).is_ok());
    }
}
