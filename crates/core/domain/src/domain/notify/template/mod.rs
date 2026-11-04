// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{connector::NotifyChannel, preference::NotifyCategory};
use crate::shared::ids::Id;

mod presets;

pub use presets::{
    NOTIFY_TEMPLATE_PRESET_LANGUAGE_PREFIX, notify_template_preset_catalog, notify_template_presets,
};

/// Notify 引擎消费的品牌无关模板投影。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyTemplate {
    pub id: Id,
    pub organization_id: Id,
    pub category: NotifyCategory,
    /// 用在哪个渠道；空着是通用模板，哪个渠道都能用。
    pub channel: Option<NotifyChannel>,
    pub body: String,
    pub format: String,
    /// 服务商审核过的模板，按服务商的名字（阿里云是 `aliyun`）。只有短信与电话模板有。
    pub provider_templates: BTreeMap<String, ProviderTemplate>,
    /// 即时通讯的 App 与群机器人发成的卡片。只有即时通讯模板有；没有时照旧发文本。
    pub card: Option<NotifyCard>,
}

/// 即时通讯的 App 与群机器人发的卡片：标题、标题的颜色、字段与链接按钮，正文是模板的正文。飞书、钉钉、
/// 企业微信、Slack、Teams、Google Chat 各按自己的卡片发出。各处都可以用占位符；字段的值与链接的地址填不
/// 出来时，这个字段或链接不显示。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyCard {
    /// 卡片的标题；空着用通知的标题。
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub color: NotifyCardColor,
    #[serde(default)]
    pub fields: Vec<NotifyCardField>,
    #[serde(default)]
    pub links: Vec<NotifyCardLink>,
    /// 卡片最下面的一行小字。
    #[serde(default)]
    pub note: String,
}

/// 卡片标题的颜色。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyCardColor {
    /// 按告警的严重度：严重是红、错误是橙、警告是黄，其余是蓝；恢复了是绿。
    #[default]
    Severity,
    Blue,
    Green,
    Yellow,
    Orange,
    Red,
    Grey,
}

/// 卡片上的一个字段：名称与值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyCardField {
    pub label: String,
    pub value: String,
}

/// 卡片上的一个链接按钮：文字与地址。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyCardLink {
    pub label: String,
    pub url: String,
}

/// 服务商审核过的一个模板：阿里云的短信与语音只发这种。`code` 是服务商给的模板编号，`params` 是模板
/// 里每个变量填什么，写法与正文一样，可以用占位符。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplate {
    pub code: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// 每个变量最多填多少个字，多出的截掉；空着用服务商的默认上限。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_param_chars: Option<u32>,
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
    Escalation,
    Schedule,
    Oncall,
    Override,
}

/// 一个可插入的模板字段。`key` 是前端 i18n 的稳定标识。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyTemplateField {
    pub key: String,
    pub token: String,
    pub group: NotifyTemplateFieldGroup,
    pub categories: Vec<NotifyCategory>,
    pub example: String,
    pub event_types: Vec<String>,
}

/// 编辑器可直接套用的模板预设。`channel` 空着的是通用模板的预设；`event_type` 是照着哪种事件写的，
/// 编辑器按它预览。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyTemplatePreset {
    pub key: String,
    pub category: NotifyCategory,
    pub channel: Option<NotifyChannel>,
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
/// 使用通用 `event.*` / `message.*` 字段。名字以 `_at` 结尾的是微秒时间戳，
/// 以 `_time` 结尾的是写给人看的时间（UTC）。
pub fn notify_template_field_catalog() -> Vec<NotifyTemplateField> {
    use NotifyCategory::{Alert, Escalation, Oncall, Report, Security, System};
    use NotifyTemplateFieldGroup::{
        Annotations, Escalation as EscalationGroup, Event, Incident, Labels, Message,
        Oncall as OncallGroup, Override, Rule, Schedule, Trigger,
    };

    let all = [Alert, Oncall, Escalation, Report, Security, System];
    let alert_categories = [Alert, Escalation];

    let alert_events = [
        "alert.triggered",
        "alert.acknowledged",
        "alert.resolved",
        "alert.escalated",
        "incident.status_update",
    ];

    let oncall_events = [
        "oncall.shift.starting",
        "oncall.shift.started",
        "oncall.override.created",
        "oncall.coverage.missing",
        "oncall.swap.updated",
        "oncall.member.unreachable",
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
            "event_time",
            "{{event.time}}",
            Event,
            &all,
            "2026-07-29 00:00:00 UTC",
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
            "incident_url",
            "{{incident.url}}",
            Incident,
            &alert_categories,
            "https://molesignal.example.com/alerts/incidents/inc_01H...",
            &alert_events,
        ),
        field(
            "incident_started_time",
            "{{incident.started_time}}",
            Incident,
            &alert_categories,
            "2026-07-28 23:55:00 UTC",
            &alert_events,
        ),
        field(
            "incident_duration",
            "{{incident.duration}}",
            Incident,
            &alert_categories,
            "1h 5m",
            &alert_events,
        ),
        field(
            "incident_acknowledged_time",
            "{{incident.acknowledged_time}}",
            Incident,
            &alert_categories,
            "2026-07-29 00:10:00 UTC",
            &["alert.acknowledged", "alert.resolved"],
        ),
        field(
            "incident_acknowledged_by",
            "{{incident.acknowledged_by}}",
            Incident,
            &alert_categories,
            "user_01H...",
            &["alert.acknowledged", "alert.resolved"],
        ),
        field(
            "incident_resolved_time",
            "{{incident.resolved_time}}",
            Incident,
            &alert_categories,
            "2026-07-29 01:00:00 UTC",
            &["alert.resolved"],
        ),
        field(
            "incident_resolved_by",
            "{{incident.resolved_by}}",
            Incident,
            &alert_categories,
            "user_01H...",
            &["alert.resolved"],
        ),
        field(
            "incident_assignees",
            "{{incident.assignees}}",
            Incident,
            &alert_categories,
            "user_01H..., user_01J...",
            &alert_events,
        ),
        field(
            "service",
            "{{service}}",
            Incident,
            &alert_categories,
            "api",
            &alert_events,
        ),
        field(
            "environment",
            "{{environment}}",
            Incident,
            &alert_categories,
            "production",
            &alert_events,
        ),
        field(
            "affected_services",
            "{{affected_services}}",
            Incident,
            &alert_categories,
            "api, checkout",
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
            "urgency",
            "{{urgency}}",
            Incident,
            &alert_categories,
            "high",
            &alert_events,
        ),
        field(
            "priority",
            "{{priority}}",
            Incident,
            &alert_categories,
            "P2",
            &alert_events,
        ),
        field(
            "urgency_name",
            "{{urgency_name}}",
            Incident,
            &alert_categories,
            "Business hours",
            &alert_events,
        ),
        field(
            "priority_name",
            "{{priority_name}}",
            Incident,
            &alert_categories,
            "Major",
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
            "labels_all",
            "{{labels}}",
            Labels,
            &alert_categories,
            "service=api, env=production",
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
        // 收回复的短信发出时才填上（见 `notify::reply`），别处原样留下。
        field(
            "reply_ack",
            "{{reply.ack}}",
            Incident,
            &alert_categories,
            "31",
            &["alert.triggered", "alert.escalated"],
        ),
        field(
            "reply_resolve",
            "{{reply.resolve}}",
            Incident,
            &alert_categories,
            "32",
            &["alert.triggered", "alert.escalated"],
        ),
        field(
            "escalation_step",
            "{{escalation.step}}",
            EscalationGroup,
            &[Escalation],
            "2",
            &["alert.escalated"],
        ),
        field(
            "escalation_loop",
            "{{escalation.loop}}",
            EscalationGroup,
            &[Escalation],
            "1",
            &["alert.escalated"],
        ),
        field(
            "escalation_target",
            "{{escalation.target}}",
            EscalationGroup,
            &[Escalation],
            "schedule",
            &["alert.escalated"],
        ),
        field(
            "escalation_reason",
            "{{escalation.reason}}",
            EscalationGroup,
            &[Escalation],
            "step",
            &["alert.escalated"],
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
            "oncall_transition_time",
            "{{oncall.transition_time}}",
            OncallGroup,
            &[Oncall],
            "2026-07-29 00:30:00 UTC",
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
            "override_start_time",
            "{{override.start_time}}",
            Override,
            &[Oncall],
            "2026-07-29 00:00:00 UTC",
            &["oncall.override.created"],
        ),
        field(
            "override_end_time",
            "{{override.end_time}}",
            Override,
            &[Oncall],
            "2026-07-29 02:00:00 UTC",
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
        let presets = notify_template_presets();
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

        let keys = presets
            .iter()
            .map(|preset| preset.key.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(keys.len(), presets.len(), "preset keys are unique");

        assert!(
            presets
                .iter()
                .filter(|preset| matches!(
                    preset.channel,
                    Some(NotifyChannel::Sms | NotifyChannel::Phone)
                ))
                .all(|preset| preset.format == "text")
        );
    }
}
