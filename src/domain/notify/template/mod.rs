// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::preference::NotifyCategory;
use crate::shared::ids::Id;

mod presets;

pub use presets::notify_template_preset_catalog;

/// Notify 引擎消费的品牌无关模板投影。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyTemplate {
    pub id: Id,
    pub organization_id: Id,
    pub category: NotifyCategory,
    pub body: String,
    pub format: String,
}

/// 模板字段在编辑器中的分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyTemplateFieldGroup {
    Event,
    Message,
    Rule,
    Incident,
    Trigger,
    Labels,
    Annotations,
    Schedule,
    Oncall,
    Override,
}

/// 一个可插入的模板字段。`key` 是前端 i18n 的稳定标识。
#[derive(Debug, Clone, Serialize)]
pub struct NotifyTemplateField {
    pub key: String,
    pub token: String,
    pub group: NotifyTemplateFieldGroup,
    pub categories: Vec<NotifyCategory>,
    pub example: String,
    pub event_types: Vec<String>,
}

/// 编辑器可直接套用的模板预设。
#[derive(Debug, Clone, Serialize)]
pub struct NotifyTemplatePreset {
    pub key: String,
    pub category: NotifyCategory,
    pub event_type: String,
    pub format: String,
    pub body: String,
}

fn field(
    key: &str,
    token: &str,
    group: NotifyTemplateFieldGroup,
    categories: &[NotifyCategory],
    example: &str,
    event_types: &[&str],
) -> NotifyTemplateField {
    NotifyTemplateField {
        key: key.into(),
        token: token.into(),
        group,
        categories: categories.to_vec(),
        example: example.into(),
        event_types: event_types.iter().map(|value| (*value).into()).collect(),
    }
}

/// 模板编辑器与后端渲染共同遵循的字段目录。
///
/// 告警模板使用 `rule.*`、`incident.*`、`evaluated_at` 等正式占位符；
/// 值班排班使用 `schedule.*`、`oncall.*` 与 `override.*`。所有类别都可
/// 使用通用 `event.*` / `message.*` 字段。
pub fn notify_template_field_catalog() -> Vec<NotifyTemplateField> {
    use NotifyCategory::{Alert, Escalation, Oncall, Report, Security, System};
    use NotifyTemplateFieldGroup::{
        Annotations, Event, Incident, Labels, Message, Oncall as OncallGroup, Override, Rule,
        Schedule, Trigger,
    };

    let all = [Alert, Oncall, Escalation, Report, Security, System];
    let alert_categories = [Alert, Escalation];
    let alert_events = [
        "alert.triggered",
        "alert.acknowledged",
        "alert.resolved",
        "alert.escalated",
    ];
    let oncall_events = [
        "oncall.shift.starting",
        "oncall.shift.started",
        "oncall.override.created",
        "oncall.coverage.missing",
    ];
    vec![
        field(
            "event_id",
            "{{event.id}}",
            Event,
            &all,
            "alert.triggered:01H...",
            &[],
        ),
        field(
            "event_type",
            "{{event.type}}",
            Event,
            &all,
            "alert.triggered",
            &[],
        ),
        field(
            "event_occurred_at",
            "{{event.occurred_at}}",
            Event,
            &all,
            "1785283200000000",
            &[],
        ),
        field(
            "message_title",
            "{{message.title}}",
            Message,
            &all,
            "CRITICAL · API unavailable",
            &[],
        ),
        field(
            "message_text",
            "{{message.text}}",
            Message,
            &all,
            "Alert API unavailable is open.",
            &[],
        ),
        field(
            "event_attribute",
            "{{event.attributes.<key>}}",
            Event,
            &all,
            "{{event.attributes.summary}}",
            &[],
        ),
        field(
            "rule_id",
            "{{rule.id}}",
            Rule,
            &alert_categories,
            "rule_01H...",
            &alert_events,
        ),
        field(
            "rule_name",
            "{{rule.name}}",
            Rule,
            &alert_categories,
            "High error rate · api",
            &alert_events,
        ),
        field(
            "rule_description",
            "{{rule.description}}",
            Rule,
            &alert_categories,
            "Error rate exceeds the SLO.",
            &alert_events,
        ),
        field(
            "incident_id",
            "{{incident.id}}",
            Incident,
            &alert_categories,
            "inc_01H...",
            &alert_events,
        ),
        field(
            "incident_fingerprint",
            "{{incident.fingerprint}}",
            Incident,
            &alert_categories,
            "api-unavailable",
            &alert_events,
        ),
        field(
            "incident_status",
            "{{incident.status}}",
            Incident,
            &alert_categories,
            "open",
            &alert_events,
        ),
        field(
            "incident_summary",
            "{{incident.summary}}",
            Incident,
            &alert_categories,
            "API unavailable",
            &alert_events,
        ),
        field(
            "severity",
            "{{severity}}",
            Incident,
            &alert_categories,
            "critical",
            &alert_events,
        ),
        field(
            "value",
            "{{value}}",
            Trigger,
            &alert_categories,
            "512",
            &alert_events,
        ),
        field(
            "threshold",
            "{{threshold}}",
            Trigger,
            &alert_categories,
            "500",
            &alert_events,
        ),
        field(
            "evaluated_at",
            "{{evaluated_at}}",
            Trigger,
            &alert_categories,
            "1785283200000000",
            &alert_events,
        ),
        field(
            "labels_key",
            "{{labels.<key>}}",
            Labels,
            &alert_categories,
            "{{labels.service}}",
            &alert_events,
        ),
        field(
            "annotations_key",
            "{{annotations.<key>}}",
            Annotations,
            &alert_categories,
            "{{annotations.runbook_url}}",
            &alert_events,
        ),
        field(
            "schedule_id",
            "{{schedule.id}}",
            Schedule,
            &[Oncall],
            "schedule_01H...",
            &oncall_events,
        ),
        field(
            "schedule_name",
            "{{schedule.name}}",
            Schedule,
            &[Oncall],
            "Primary",
            &oncall_events,
        ),
        field(
            "schedule_team_id",
            "{{schedule.team_id}}",
            Schedule,
            &[Oncall],
            "team_01H...",
            &oncall_events,
        ),
        field(
            "schedule_timezone",
            "{{schedule.timezone}}",
            Schedule,
            &[Oncall],
            "Asia/Shanghai",
            &oncall_events,
        ),
        field(
            "oncall_current_user",
            "{{oncall.current_user_id}}",
            OncallGroup,
            &[Oncall],
            "user_current",
            &["oncall.shift.starting", "oncall.shift.started"],
        ),
        field(
            "oncall_next_user",
            "{{oncall.next_user_id}}",
            OncallGroup,
            &[Oncall],
            "user_next",
            &["oncall.shift.starting", "oncall.shift.started"],
        ),
        field(
            "oncall_transition_at",
            "{{oncall.transition_at}}",
            OncallGroup,
            &[Oncall],
            "1785285000000000",
            &["oncall.shift.starting", "oncall.shift.started"],
        ),
        field(
            "override_id",
            "{{override.id}}",
            Override,
            &[Oncall],
            "override_01H...",
            &["oncall.override.created"],
        ),
        field(
            "override_user",
            "{{override.user_id}}",
            Override,
            &[Oncall],
            "user_substitute",
            &["oncall.override.created"],
        ),
        field(
            "override_original_user",
            "{{override.original_user_id}}",
            Override,
            &[Oncall],
            "user_original",
            &["oncall.override.created"],
        ),
        field(
            "override_start_at",
            "{{override.start_at}}",
            Override,
            &[Oncall],
            "1785283200000000",
            &["oncall.override.created"],
        ),
        field(
            "override_end_at",
            "{{override.end_at}}",
            Override,
            &[Oncall],
            "1785290400000000",
            &["oncall.override.created"],
        ),
        field(
            "override_reason",
            "{{override.reason}}",
            Override,
            &[Oncall],
            "Temporary coverage",
            &["oncall.override.created"],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_alert_escalation_and_oncall_templates() {
        let fields = notify_template_field_catalog();
        let rule_name = fields
            .iter()
            .find(|field| field.token == "{{rule.name}}")
            .expect("rule name field");
        assert!(rule_name.categories.contains(&NotifyCategory::Escalation));
        assert!(
            fields
                .iter()
                .any(|field| field.token == "{{schedule.name}}")
        );
        assert!(
            fields
                .iter()
                .any(|field| field.token == "{{override.reason}}")
        );
        let presets = notify_template_preset_catalog();
        for category in [
            NotifyCategory::Alert,
            NotifyCategory::Oncall,
            NotifyCategory::Report,
            NotifyCategory::Escalation,
            NotifyCategory::Security,
            NotifyCategory::System,
        ] {
            for format in ["text", "markdown", "html"] {
                assert!(
                    presets
                        .iter()
                        .any(|preset| { preset.category == category && preset.format == format }),
                    "missing {} preset for {}",
                    format,
                    category.as_str()
                );
            }
        }
    }
}
