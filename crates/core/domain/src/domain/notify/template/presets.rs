// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::NotifyTemplatePreset;
use crate::domain::{
    i18n_language::LanguagePack,
    notify::{connector::NotifyChannel, preference::NotifyCategory},
};

/// 预置模板的正文按语言放在语言表里，键是这个前缀加上预置模板的 `key`。
pub const NOTIFY_TEMPLATE_PRESET_LANGUAGE_PREFIX: &str = "notify.template.preset.";

fn preset(
    key: &str,
    category: NotifyCategory,
    event_type: &str,
    format: &str,
) -> NotifyTemplatePreset {
    NotifyTemplatePreset {
        key: key.into(),
        category,
        channel: None,
        event_type: event_type.into(),
        format: format.into(),
        body: String::new(),
    }
}

/// 只给一个渠道的模板用的预设。
fn only_for(channel: NotifyChannel, preset: NotifyTemplatePreset) -> NotifyTemplatePreset {
    NotifyTemplatePreset {
        channel: Some(channel),
        ..preset
    }
}

/// 短信或电话模板的预设：一段纯文本，短到一条短信、一通电话说得完。
fn plain(
    key: &str,
    category: NotifyCategory,
    channel: NotifyChannel,
    event_type: &str,
) -> NotifyTemplatePreset {
    only_for(channel, preset(key, category, event_type, "text"))
}

/// 编辑器可直接套用的模板预设，正文取自 `pack`（语言表里
/// [`NOTIFY_TEMPLATE_PRESET_LANGUAGE_PREFIX`] 下的条目）；语言表里没有正文的不列出。
pub fn notify_template_preset_catalog(pack: &LanguagePack) -> Vec<NotifyTemplatePreset> {
    notify_template_presets()
        .into_iter()
        .filter_map(|mut preset| {
            let key = format!("{NOTIFY_TEMPLATE_PRESET_LANGUAGE_PREFIX}{}", preset.key);
            preset.body = pack.get(&key)?.to_string();
            Some(preset)
        })
        .collect()
}

/// 全部预设，正文还空着：写在哪个类别、渠道与格式下，照着哪种事件写的。
pub fn notify_template_presets() -> Vec<NotifyTemplatePreset> {
    use NotifyCategory::{Alert, Escalation, Oncall, Report, Security, System};
    use NotifyChannel::{Email, Im, Phone, Sms};

    vec![
        plain("alert_sms", Alert, Sms, "alert.triggered"),
        plain("alert_resolved_sms", Alert, Sms, "alert.resolved"),
        plain("alert_phone", Alert, Phone, "alert.triggered"),
        plain("escalation_sms", Escalation, Sms, "alert.escalated"),
        plain("escalation_phone", Escalation, Phone, "alert.escalated"),
        plain("oncall_sms", Oncall, Sms, "oncall.shift.starting"),
        plain(
            "oncall_coverage_sms",
            Oncall,
            Sms,
            "oncall.coverage.missing",
        ),
        plain("oncall_phone", Oncall, Phone, "oncall.shift.starting"),
        only_for(
            Im,
            preset("alert_im_markdown", Alert, "alert.triggered", "markdown"),
        ),
        only_for(
            Email,
            preset("alert_email_html", Alert, "alert.triggered", "html"),
        ),
        preset("alert_text", Alert, "alert.triggered", "text"),
        preset("alert_markdown", Alert, "alert.triggered", "markdown"),
        preset("alert_html", Alert, "alert.triggered", "html"),
        preset(
            "alert_acknowledged_markdown",
            Alert,
            "alert.acknowledged",
            "markdown",
        ),
        preset("alert_resolved_text", Alert, "alert.resolved", "text"),
        preset(
            "alert_resolved_markdown",
            Alert,
            "alert.resolved",
            "markdown",
        ),
        preset("oncall_shift", Oncall, "oncall.shift.starting", "markdown"),
        preset(
            "oncall_override",
            Oncall,
            "oncall.override.created",
            "markdown",
        ),
        preset("oncall_coverage", Oncall, "oncall.coverage.missing", "text"),
        preset("oncall_html", Oncall, "oncall.shift.starting", "html"),
        preset("escalation_text", Escalation, "alert.escalated", "text"),
        preset(
            "escalation_markdown",
            Escalation,
            "alert.escalated",
            "markdown",
        ),
        preset("escalation_html", Escalation, "alert.escalated", "html"),
        preset("report_text", Report, "report.ready", "text"),
        preset("report_markdown", Report, "report.ready", "markdown"),
        preset("report_html", Report, "report.ready", "html"),
        preset(
            "security_text",
            Security,
            "security.access.detected",
            "text",
        ),
        preset(
            "security_markdown",
            Security,
            "security.access.detected",
            "markdown",
        ),
        preset(
            "security_html",
            Security,
            "security.access.detected",
            "html",
        ),
        preset("system_text", System, "system.health.changed", "text"),
        preset(
            "system_markdown",
            System,
            "system.health.changed",
            "markdown",
        ),
        preset("system_html", System, "system.health.changed", "html"),
    ]
}
